#![feature(async_await, await_macro, futures_api)]

#[macro_use]
extern crate index_repo;

use std::env;
use std::fs::File;
use std::io::Read;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use bytes::buf::Buf;
use clap::{app_from_crate, Arg, crate_authors, crate_description, crate_name, crate_version};
use diesel::prelude::*;
use diesel_migrations::run_pending_migrations;
use dotenv::dotenv;
use failure::{Error, format_err, ResultExt};
use futures::future::join_all;
use futures::Stream;
use log::{debug, info, warn};
use tokio_executor::DefaultExecutor;
use tokio_sync::semaphore::Semaphore;

use index_repo::cpio;
use index_repo::db;
use index_repo::decoders::Decoder;
use index_repo::errors::FutureExt;
use index_repo::fs::create_file_all;
use index_repo::hashes;
use index_repo::http;
use index_repo::models::*;
use index_repo::repomd;
use index_repo::rpm;

async fn fetch_repomd<'a>(
    client: &'a http::Client,
    semaphore: &'a Semaphore,
    repomd_uri: hyper::Uri,
) -> Result<repomd::Document, Error> {
    let response = await!(http::checked_fetch(client, semaphore, repomd_uri.clone()))?;
    let body = await_old!(response
        .into_body()
        .concat2()
        .with_context(move |_| format!(
            "Failed to fetch {}: failed to read response body", repomd_uri)))?;
    repomd::Document::parse(body.reader())
}

async fn fetch_file<'a>(
    client: &'a http::Client,
    http_semaphore: &'a Semaphore,
    repo_uri: String,
    href: String,
    open_checksum: repomd::Checksum,
) -> Result<PathBuf, Error> {
    let decoder = Decoder::from_href(&href);
    let path = decoder.path().to_owned();
    debug!("Hashing file {}...", path.to_string_lossy());
    if let Ok(hexdigest) = hashes::hexdigest_path(&path, &open_checksum.tpe) {
        if hexdigest == open_checksum.hexdigest {
            return Ok(path);
        }
    };
    let uri_str = repo_uri.to_owned() + "/" + &href;
    let uri = uri_str.parse::<hyper::Uri>()
        .with_context(|_| format!("Malformed URI: {}", uri_str))?;
    let file = create_file_all(&path)?;
    let response = await!(http::checked_fetch(client, http_semaphore, uri))?;
    await_old!(decoder.decode_response(file, response))?;
    Ok(path)
}

fn index_elf_file(
    conn: &Mutex<SqliteConnection>,
    file_id: i32,
    mut file: File,
) -> Result<(), Error> {
    let mut elf_bytes = Vec::new();
    file.read_to_end(&mut elf_bytes)?;
    let elf = match goblin::Object::parse(&elf_bytes) {
        Ok(goblin::Object::Elf(t)) => t,
        _ => return Ok(()), // ignore errors - peek() could have been mistaken
    };
    for sym in elf.dynsyms.iter() {
        let conn = &*conn.lock().map_err(|_| format_err!("Failed to lock a SqliteConnection"))?;
        let _symbol_id = match db::persist_elf_symbol(conn, file_id, &elf.dynstrtab, &sym) {
            Ok(t) => t,
            Err(e) => {
                warn!("{}", index_repo::errors::format(&e));
                continue;
            }
        };
    }
    Ok(())
}

fn index_file(
    conn: &Mutex<SqliteConnection>,
    package_id: i32,
    name: &str,
    mut file: File,
) -> Result<(), Error> {
    let file_id = {
        let conn = &*conn.lock().map_err(|_| format_err!("Failed to lock a SqliteConnection"))?;
        db::persist_file(conn, package_id, name)?
    };
    match goblin::peek(&mut file) {
        Ok(goblin::Hint::Elf(_)) => index_elf_file(conn, file_id, file),
        _ => Ok(()),  // ignore errors, because peek() fails on small files
    }
}

async fn index_package(
    conn: Arc<Mutex<SqliteConnection>>,
    repo_id: i32,
    client: http::Client,
    http_semaphore: Arc<Semaphore>,
    repo_uri: String,
    p: RpmPackage,
) -> Result<(), Error> {
    let path = await!(fetch_file(
        &client,
        &http_semaphore,
        repo_uri.clone(),
        p.location_href.clone(),
        repomd::Checksum {
            tpe: p.checksum_type.to_owned(),
            hexdigest: p.pkg_id.to_owned(),
        }))?;
    info!("Indexing package {}/{}...", &repo_uri, &p.location_href);
    let file = await_old!(tokio::fs::File::open(path.clone())
        .with_context(move |_| format!("Could not open {:?}", path)))?;
    let package_id = {
        let conn = &*conn.lock().map_err(|_| format_err!("Failed to lock a SqliteConnection"))?;
        db::persist_package(conn, repo_id, &p)?
    };
    let (mut a, _pos, _lead, _signature_header, _header) = await!(rpm::read_all_headers(file))?;
    let mut pos = 0;
    loop {
        let (local_a, local_pos, entry) = await!(cpio::read_entry(a, pos))?;
        let (_cpio_header, cpio_name, cpio_data) = match entry {
            Some(t) => t,
            None => break Ok(()),
        };
        debug!("Indexing file {}/{}:{}...", &repo_uri, &p.location_href, &cpio_name);
        if let Err(e) = index_file(&conn, package_id, &cpio_name, cpio_data) {
            warn!("{}", e);
        }
        a = local_a;
        pos = local_pos;
    }
}

async fn index_repo(
    conn: SqliteConnection,
    client: http::Client,
    repo_uri: String,
    arches: Option<Vec<String>>,
    requirements: Option<Vec<String>>,
    jobs: usize,
) -> Result<(), Error> {
    info!("Indexing repo {}...", &repo_uri);
    let http_semaphore = Arc::new(Semaphore::new(jobs));
    let repomd_uri_str = repo_uri.to_owned() + "/repodata/repomd.xml";
    let repomd_uri = repomd_uri_str.parse::<hyper::Uri>()
        .context(format!("Malformed URI: {}", repomd_uri_str))?;
    let doc = await!(fetch_repomd(&client, &http_semaphore, repomd_uri))?;
    let primary_db_data = doc.data
        .iter()
        .find(|data| data.tpe == "primary_db")
        .ok_or_else(|| format_err!(r#"Missing <data type="primary_db">"#))?;
    let repo_id = db::persist_repo(&conn, &repo_uri, &primary_db_data)?;
    let open_checksum = primary_db_data.open_checksum
        .as_ref()
        .ok_or_else(|| format_err!("Missing <open-checksum>"))?
        .clone();
    let primary_db_path = await!(fetch_file(
        &client,
        &http_semaphore,
        repo_uri.clone(),
        primary_db_data.location.href.clone(),
        open_checksum))?;
    info!("Reading package lists...");
    let packages = db::get_packages(&primary_db_path, &arches, &requirements)?;
    let packages_size: i64 = packages.iter().map(|p| p.size_package as i64).sum();
    info!("Total size of RPMs: {}", pretty_bytes::converter::convert(packages_size as f64));
    let conn = Arc::new(Mutex::new(conn));
    let index_packages = join_all(packages
        .into_iter()
        .map(move |package| {
            let future = index_package(
                conn.clone(),
                repo_id,
                client.clone(),
                http_semaphore.clone(),
                repo_uri.clone(),
                package);
            let compat_future = tokio_async_await::compat::backward::Compat::new(future);
            futures::sync::oneshot::spawn(compat_future, &DefaultExecutor::current())
        }));
    await_old!(index_packages)?;
    Ok(())
}

async fn bootstrap() -> Result<(), Error> {
    dotenv().ok();
    let matches = app_from_crate!()
        .arg(Arg::with_name("DATABASE_URL")
            .long("database-url")
            .takes_value(true))
        .arg(Arg::with_name("ARCH")
            .long("arch")
            .number_of_values(1)
            .multiple(true))
        .arg(Arg::with_name("REQUIRES")
            .long("requires")
            .number_of_values(1)
            .multiple(true))
        .arg(Arg::with_name("JOBS")
            .short("j")
            .long("jobs")
            .default_value("1"))
        .arg(Arg::with_name("URI")
            .required(true)
            .index(1))
        .get_matches();
    let database_url = matches
        .value_of("DATABASE_URL")
        .map(std::borrow::ToOwned::to_owned)
        .or_else(|| { env::var("DATABASE_URL").ok() })
        .unwrap_or_else(|| "index.sqlite".to_owned());
    let arches = matches.values_of_lossy("ARCH");
    let requirements = matches.values_of_lossy("REQUIRES");
    let jobs = matches.value_of("JOBS").unwrap().parse::<usize>()
        .context("Malformed -j/--jobs value")?;
    let repo_uri = matches.value_of("URI").unwrap();
    let conn = SqliteConnection::establish(&database_url)
        .context(format!("SqliteConnection::establish({}) failed", database_url))?;
    run_pending_migrations(&conn)
        .context("run_pending_migrations() failed")?;
    let client = http::make_client()?;
    await!(index_repo(conn, client, repo_uri.to_owned(), arches, requirements, jobs))
}

fn main() -> Result<(), Error> {
    env_logger::init();
    index_repo::tokio::main(tokio_async_await::compat::backward::Compat::new(bootstrap()))
}
