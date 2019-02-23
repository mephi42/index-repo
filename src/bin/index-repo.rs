extern crate bytes;
extern crate clap;
extern crate diesel;
extern crate diesel_migrations;
extern crate dotenv;
extern crate error_chain;
extern crate futures;
extern crate hyper;
#[macro_use]
extern crate index_repo;
extern crate itertools;
extern crate smallvec;
extern crate tempfile;
extern crate tokio;
extern crate tokio_io;
extern crate xz2;

use std::env;
use std::path::{Path, PathBuf};

use bytes::buf::Buf;
use clap::{App, Arg};
use diesel::dsl::exists;
use diesel::prelude::*;
use diesel::query_source::joins::{Inner, Join};
use diesel::sql_types;
use diesel_migrations::run_pending_migrations;
use dotenv::dotenv;
use error_chain::ChainedError;
use futures::future::{done, failed, ok};
use futures::Stream;
use futures::stream::iter_ok;
use hyper::rt;
use hyper::rt::Future;
use itertools::Itertools;
use smallvec::SmallVec;
use tokio_io::AsyncRead;
use xz2::read::XzDecoder;

use index_repo::cpio;
use index_repo::decoders::Decoder;
use index_repo::errors::*;
use index_repo::fs::create_file_all;
use index_repo::hashes;
use index_repo::http;
use index_repo::models::*;
use index_repo::repomd;
use index_repo::rpm;
use index_repo::schema::*;

fn fetch_repomd(client: &http::Client, repomd_uri: &hyper::Uri)
                -> impl Future<Item=repomd::Document, Error=Error> {
    http::checked_fetch(client, repomd_uri)
        .and_then({
            let repomd_uri = repomd_uri.clone();
            move |response| {
                response.into_body().concat2().chain_err({
                    let repomd_uri = repomd_uri.clone();
                    move || format!(
                        "Failed to fetch {}: failed to read response body", repomd_uri)
                })
            }
        })
        .and_then(|body| done(repomd::Document::parse(body.reader())))
}

fn persist_repomd(conn: &SqliteConnection, repo_uri: &str, primary_db_data: &repomd::Data)
                  -> Result<()> {
    conn.transaction(|| {
        let repo_vec = repos::table
            .filter(repos::uri.eq(repo_uri))
            .limit(1)
            .load::<Repo>(conn)
            .chain_err(|| "Failed to query repo by uri")?;
        match repo_vec.first() {
            Some(repo) =>
                diesel::update(repos::table.filter(repos::id.eq(repo.id)))
                    .set(repos::primary_db.eq(&primary_db_data.location.href))
                    .execute(conn)
                    .chain_err(|| "Failed to update repo")?,
            None =>
                diesel::insert_into(repos::table)
                    .values((
                        repos::uri.eq(repo_uri),
                        repos::primary_db.eq(&primary_db_data.location.href)))
                    .execute(conn)
                    .chain_err(|| "Failed to insert repo")?,
        };
        Ok(())
    })
}

fn fetch_file(client: &http::Client, repo_uri: &str, href: &str, open_checksum: &repomd::Checksum)
              -> Box<Future<Item=PathBuf, Error=Error> + Send> {
    let decoder = Decoder::from_href(href);
    let path = decoder.path().to_owned();
    if let Ok(hexdigest) = hashes::hexdigest_path(&path, &open_checksum.tpe) {
        if hexdigest == open_checksum.hexdigest {
            return Box::new(ok(path));
        }
    };
    let uri_str = repo_uri.to_owned() + "/" + href;
    let uri = try_future!(uri_str.parse().chain_err(|| format!("Malformed URI: {}", uri_str)));
    let file = try_future!(create_file_all(&path));
    Box::new(http::checked_fetch(client, &uri)
        .and_then(move |response| { decoder.decode_response(file, response) })
        .and_then(|()| ok(path)))
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

fn get_package_hrefs(path: &Path, arches: &Option<Vec<String>>, requirements: &Option<Vec<String>>)
                     -> Result<Vec<Package>> {
    let database_url = "file:".to_owned() +
        path.to_str().chain_err(|| format!("Malformed path: {:?}", path))? +
        "?mode=ro";
    let conn = SqliteConnection::establish(&database_url)
        .chain_err(|| format!(
            "SqliteConnection::establish({}) failed", database_url))?;
    let mut query = packages::table.into_boxed();
    if let Some(requirements) = requirements {
        // https://stackoverflow.com/a/48712715/3832536
        // https://github.com/diesel-rs/diesel/issues/1544#issuecomment-363440046
        type B = Box<BoxableExpression<
            Join<requires::table, packages::table, Inner>,
            diesel::sqlite::Sqlite,
            SqlType=sql_types::Bool>>;
        let like: B = requirements
            .iter()
            .map(|r| -> B {
                Box::new(requires::name.like(like_from_wildcard(r)))
            }).fold1(|q, l| Box::new(q.or(l)))
            .unwrap();
        query = query.filter(exists(requires::table.filter(
            requires::pkgKey.eq(packages::pkgKey).and(like))));
    }
    if let Some(arches) = arches {
        query = query.filter(packages::arch.eq_any(arches));
    }
    query.load::<Package>(&conn).chain_err(|| "Failed to query packages")
}

fn index_package(client: &http::Client, repo_uri: &str, p: &Package)
                 -> Box<Future<Item=(), Error=Error> + Send> {
    Box::new(fetch_file(client, repo_uri, &p.location_href, &repomd::Checksum {
        tpe: p.checksum_type.to_owned(),
        hexdigest: p.pkg_id.to_owned(),
    }).and_then(|path| {
        tokio::fs::File::open(path.clone())
            .chain_err(move || format!("Could not open {:?}", path))
    }).and_then(|file| {
        rpm::read_lead(file, 0)
    }).and_then(|(file, pos, _rpm_lead)| {
        rpm::read_full_header(file, pos)
    }).and_then(|(file, pos, _rpm_signature_header)| {
        rpm::read_full_header(file, pos)
    }).and_then(|(file, pos, rpm_header)| -> cpio::ReadHeader<_> {
        let format = try_future!(rpm_header.get_string_tag(1124, "cpio"));
        if format != "cpio" {
            return Box::new(failed("Unsupported RPM payload format".into()));
        }
        let coding = try_future!(rpm_header.get_string_tag(1125, "gzip"));
        let r: Box<AsyncRead + Send> = match coding.as_ref() {
            "xz" => Box::new(XzDecoder::new(file)),
            _ => return Box::new(failed("Unsupported RPM payload coding".into())),
        };
        cpio::read_header(r, pos)
    }).and_then(|(_r, _pos, _cpio_header)| {
        ok(())
    }))
}

fn bootstrap() -> Box<Future<Item=(), Error=Error> + Send> {
    dotenv().ok();
    let matches = App::new(env!("CARGO_PKG_NAME"))
        .version(env!("CARGO_PKG_VERSION"))
        .author(env!("CARGO_PKG_AUTHORS"))
        .about(env!("CARGO_PKG_DESCRIPTION"))
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
        .map(|x| x.to_owned())
        .or_else(|| { env::var("DATABASE_URL").ok() })
        .unwrap_or_else(|| "index.sqlite".to_owned());
    let arches = matches.values_of_lossy("ARCH");
    let requirements = matches.values_of_lossy("REQUIRES");
    let jobs = try_future!(matches.value_of("JOBS").unwrap().parse::<usize>()
        .chain_err(|| "Malformed -j/--jobs value"));
    let repo_uri = matches.value_of("URI").unwrap();
    let conn = try_future!(SqliteConnection::establish(&database_url)
        .chain_err(|| format!("SqliteConnection::establish({}) failed", database_url)));
    try_future!(run_pending_migrations(&conn)
        .chain_err(|| "run_pending_migrations() failed"));
    let repomd_uri_str = repo_uri.to_owned() + "/repodata/repomd.xml";
    let repomd_uri = try_future!(repomd_uri_str.parse()
        .chain_err(|| format!("Malformed URI: {}", repomd_uri_str)));
    let client = try_future!(http::make_client());
    Box::new(fetch_repomd(&client, &repomd_uri)
        .and_then({
            let repo_uri = repo_uri.to_owned();
            move |doc| -> Box<Future<Item=(http::Client, PathBuf), Error=Error> + Send> {
                let primary_db_data = try_future!(doc.data
                    .iter()
                    .find(|data| data.tpe == "primary_db")
                    .ok_or_else(|| r#"Missing <data type="primary_db">"#.into()));
                try_future!(persist_repomd(&conn, &repo_uri, &primary_db_data));
                let open_checksum = try_future!(primary_db_data.open_checksum
                    .as_ref()
                    .ok_or_else(|| "Missing <open-checksum>".into()));
                let primary_db_href = &primary_db_data.location.href;
                Box::new(fetch_file(&client, &repo_uri, primary_db_href, &open_checksum)
                    .map(|path| (client, path)))
            }
        })
        .and_then({
            let repo_uri = repo_uri.to_owned();
            move |(client, path)| -> Box<Future<Item=(), Error=Error> + Send> {
                let package_hrefs = try_future!(get_package_hrefs(&path, &arches, &requirements));
                Box::new(iter_ok(package_hrefs)
                    .map(move |p| index_package(&client, &repo_uri, &p))
                    .buffer_unordered(jobs)
                    .collect()
                    .map(|_| ()))
            }
        }))
}

fn main() {
    rt::run(Box::new(bootstrap().map_err(|e| {
        eprintln!("{}", e.display_chain());
        std::process::exit(1);
    })));
}
