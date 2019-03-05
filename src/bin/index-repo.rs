#![feature(async_await, await_macro, futures_api)]

#[macro_use]
extern crate index_repo;

use std::env;
use std::fs::File;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use bytes::buf::Buf;
use clap::{app_from_crate, Arg, crate_authors, crate_description, crate_name, crate_version};
use diesel::dsl::exists;
use diesel::prelude::*;
use diesel::query_source::joins::{Inner, Join};
use diesel::sql_types;
use diesel_migrations::run_pending_migrations;
use dotenv::dotenv;
use failure::{bail, Error, format_err, ResultExt};
use futures::Stream;
use futures::stream::iter_ok;
use itertools::Itertools;
use log::{debug, info, warn};
use smallvec::SmallVec;

use index_repo::cpio;
use index_repo::decoders::Decoder;
use index_repo::errors::FutureExt;
use index_repo::fs::create_file_all;
use index_repo::hashes;
use index_repo::http;
use index_repo::models::*;
use index_repo::repomd;
use index_repo::rpm;
use index_repo::schema::*;

async fn fetch_repomd(
    client: &http::Client, repomd_uri: hyper::Uri,
) -> Result<repomd::Document, Error> {
    let response = await!(http::checked_fetch(client, repomd_uri.clone()))?;
    let body = await_old!(response
        .into_body()
        .concat2()
        .with_context(move |_| format!(
            "Failed to fetch {}: failed to read response body", repomd_uri)))?;
    repomd::Document::parse(body.reader())
}

macro_rules! and_all {
    ($x:expr) => {
        $x
    };
    ($x:expr, $($xs:expr),+ $(,)?) => {{
        $x.and(and_all![$($xs),*])
    }};
}

macro_rules! insert_into_returning_rowid {
    ($conn:expr, $table: expr, $rowid: expr, $desc: expr, ($($vs:expr),* $(,)?)) => {{
        diesel::insert_into($table)
            .values(($($vs,)*))
            .execute($conn)
            .context(format!("Failed to insert {}", $desc))?;
        let rows = $table
            .filter(and_all![$($vs),*])
            .select($rowid)
            .limit(1)
            .load::<i32>($conn)
            .context(format!("Failed to query {}", $desc))?;
        match rows.as_slice() {
            [rowid] => Ok(*rowid),
            _ => bail!("Could not find {}", $desc),
        }
    }}
}

fn persist_repo(
    conn: &SqliteConnection,
    repo_uri: &str,
    primary_db_data: &repomd::Data,
) -> Result<i32, Error> {
    insert_into_returning_rowid![
        conn,
        repos::table,
        repos::id,
        "a repo",
        (
            repos::uri.eq(repo_uri),
            repos::primary_db.eq(&primary_db_data.location.href),
        )]
}

async fn fetch_file(
    client: &http::Client, repo_uri: String, href: String, open_checksum: repomd::Checksum,
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
    let response = await!(http::checked_fetch(client, uri))?;
    await_old!(decoder.decode_response(file, response))?;
    Ok(path)
}

fn like_from_wildcard(s: &str) -> String {
    s.chars().flat_map(|c| {
        let mut v = SmallVec::<[char; 2]>::new();
        match c {
            '*' => v.push('%'),
            '?' => v.push('_'),
            '%' => v.extend_from_slice(&['\\', '%']),
            '_' => v.extend_from_slice(&['\\', '_']),
            '\\' => v.extend_from_slice(&['\\', '\\']),
            x => v.push(x),
        };
        v
    }).collect()
}

fn get_packages(path: &Path, arches: &Option<Vec<String>>, requirements: &Option<Vec<String>>)
                -> Result<Vec<RpmPackage>, Error> {
    info!("Reading package lists...");
    let database_url = "file:".to_owned() +
        path.to_str().ok_or_else(|| format_err!("Malformed path: {:?}", path))? +
        "?mode=ro";
    let conn = SqliteConnection::establish(&database_url)
        .with_context(|_| format!(
            "SqliteConnection::establish({}) failed", database_url))?;
    let mut query = rpm_packages::table.into_boxed();
    if let Some(requirements) = requirements {
        // https://stackoverflow.com/a/48712715/3832536
        // https://github.com/diesel-rs/diesel/issues/1544#issuecomment-363440046
        type B = Box<BoxableExpression<
            Join<rpm_requires::table, rpm_packages::table, Inner>,
            diesel::sqlite::Sqlite,
            SqlType=sql_types::Bool>>;
        let like: B = requirements
            .iter()
            .map(|r| -> B {
                Box::new(rpm_requires::name.like(like_from_wildcard(r)))
            }).fold1(|q, l| Box::new(q.or(l)))
            .unwrap();
        query = query.filter(exists(rpm_requires::table.filter(
            rpm_requires::pkgKey.eq(rpm_packages::pkgKey).and(like))));
    }
    if let Some(arches) = arches {
        query = query.filter(rpm_packages::arch.eq_any(arches));
    }
    query.load::<RpmPackage>(&conn)
        .context("Failed to query packages")
        .map_err(Error::from)
}

fn persist_string(
    conn: &SqliteConnection,
    s: &str,
) -> Result<i32, Error> {
    let query = strings::table
        .filter(strings::name.eq(s))
        .select(strings::id)
        .limit(1);
    let rows = query
        .load::<i32>(conn)
        .context(format!("Failed to query a string"))?;
    match rows.as_slice() {
        [] => {
            insert_into_returning_rowid![
                conn,
                strings::table,
                strings::id,
                "a string",
                (strings::name.eq(s))]
        }
        [rowid] => Ok(*rowid),
        _ => unreachable!(),
    }
}

fn persist_elf_symbol(
    conn: &SqliteConnection,
    file_id: i32,
    strtab: &goblin::strtab::Strtab,
    sym: &goblin::elf::Sym,
) -> Result<i32, Error> {
    let name = strtab.get(sym.st_name)
        .ok_or_else(|| format_err!(
        "Failed to resolve an ELF symbol name (st_name={:x})", sym.st_name))??;
    let name_id = persist_string(conn, name)?;
    insert_into_returning_rowid![
        conn,
        elf_symbols::table,
        elf_symbols::id,
        "an ELF symbol",
        (
            elf_symbols::file_id.eq(file_id),
            elf_symbols::name_id.eq(name_id),
            elf_symbols::st_info.eq(sym.st_info as i32),
            elf_symbols::st_other.eq(sym.st_other as i32),
        )]
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
        let _symbol_id = match persist_elf_symbol(conn, file_id, &elf.dynstrtab, &sym) {
            Ok(t) => t,
            Err(e) => {
                warn!("{}", index_repo::errors::format(&e));
                continue;
            }
        };
    }
    Ok(())
}

fn persist_file(
    conn: &SqliteConnection,
    package_id: i32,
    name: &str,
) -> Result<i32, Error> {
    insert_into_returning_rowid![
        conn,
        files::table,
        files::id,
        "a file",
        (
            files::package_id.eq(package_id),
            files::name.eq(name),
        )]
}

fn index_file(
    conn: &Mutex<SqliteConnection>,
    package_id: i32,
    name: &str,
    mut file: File,
) -> Result<(), Error> {
    let file_id = {
        let conn = &*conn.lock().map_err(|_| format_err!("Failed to lock a SqliteConnection"))?;
        persist_file(conn, package_id, name)?
    };
    match goblin::peek(&mut file) {
        Ok(goblin::Hint::Elf(_)) => index_elf_file(conn, file_id, file),
        _ => Ok(()),  // ignore errors, because peek() fails on small files
    }
}

fn persist_package(
    conn: &SqliteConnection,
    repo_id: i32,
    p: &RpmPackage,
) -> Result<i32, Error> {
    insert_into_returning_rowid![
        conn,
        packages::table,
        packages::id,
        "a package",
        (
            packages::repo_id.eq(repo_id),
            packages::name.eq(&p.name),
            packages::arch.eq(&p.arch),
            packages::version.eq(&p.version),
            packages::epoch.eq(&p.epoch),
            packages::release.eq(&p.release),
        )]
}

async fn index_package<'a>(
    conn: &'a Mutex<SqliteConnection>,
    repo_id: i32,
    client: &'a http::Client,
    repo_uri: String,
    p: RpmPackage,
) -> Result<(), Error> {
    info!("Indexing package {}/{}...", &repo_uri, &p.location_href);
    let path = await!(fetch_file(
        &client,
        repo_uri.clone(),
        p.location_href.clone(),
        repomd::Checksum {
            tpe: p.checksum_type.to_owned(),
            hexdigest: p.pkg_id.to_owned(),
        }))?;
    let file = await_old!(tokio::fs::File::open(path.clone())
        .with_context(move |_| format!("Could not open {:?}", path)))?;
    let package_id = {
        let conn = &*conn.lock().map_err(|_| format_err!("Failed to lock a SqliteConnection"))?;
        persist_package(conn, repo_id, &p)?
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
        if let Err(e) = index_file(conn, package_id, &cpio_name, cpio_data) {
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
    let repomd_uri_str = repo_uri.to_owned() + "/repodata/repomd.xml";
    let repomd_uri = repomd_uri_str.parse::<hyper::Uri>()
        .context(format!("Malformed URI: {}", repomd_uri_str))?;
    let doc = await!(fetch_repomd(&client, repomd_uri))?;
    let primary_db_data = doc.data
        .iter()
        .find(|data| data.tpe == "primary_db")
        .ok_or_else(|| format_err!(r#"Missing <data type="primary_db">"#))?;
    let repo_id = persist_repo(&conn, &repo_uri, &primary_db_data)?;
    let open_checksum = primary_db_data.open_checksum
        .as_ref()
        .ok_or_else(|| format_err!("Missing <open-checksum>"))?
        .clone();
    let primary_db_path = await!(fetch_file(
        &client,
        repo_uri.clone(),
        primary_db_data.location.href.clone(),
        open_checksum))?;
    let packages = get_packages(&primary_db_path, &arches, &requirements)?;
    let conn = Mutex::new(conn);
    let index_packages = iter_ok(packages)
        .map(|package| tokio_async_await::compat::backward::Compat::new(index_package(
            &conn, repo_id, &client, repo_uri.clone(), package)))
        .buffer_unordered(jobs)
        .collect();
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
