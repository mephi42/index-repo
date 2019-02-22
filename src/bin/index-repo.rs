extern crate bytes;
extern crate clap;
extern crate diesel;
extern crate dotenv;
extern crate error_chain;
extern crate futures;
extern crate hyper;
#[macro_use]
extern crate index_repo;
extern crate tempfile;
extern crate xz2;

use std::env;
use std::path::PathBuf;

use bytes::buf::Buf;
use clap::{App, Arg};
use diesel::prelude::*;
use dotenv::dotenv;
use error_chain::ChainedError;
use futures::future::{done, failed, ok};
use futures::Stream;
use hyper::rt;
use hyper::rt::Future;

use index_repo::decoders::Decoder;
use index_repo::errors::*;
use index_repo::fs::create_file_all;
use index_repo::hashes;
use index_repo::http;
use index_repo::models::Repo;
use index_repo::repomd;

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

fn get_primary_db(doc: &repomd::Document) -> Option<&repomd::Data> {
    doc.data.iter().find(|data| data.tpe == "primary_db")
}

fn persist_repomd(conn: &SqliteConnection, repo_uri: &str, primary_db_data: &repomd::Data)
                  -> Result<()> {
    use index_repo::schema::repos::dsl::*;
    conn.transaction(|| {
        let repo_vec = repos
            .filter(uri.eq(repo_uri))
            .limit(1)
            .load::<Repo>(conn)
            .chain_err(|| "Failed to query repo by uri")?;
        match repo_vec.first() {
            Some(repo) =>
                diesel::update(repos.filter(id.eq(repo.id)))
                    .set(primary_db.eq(&primary_db_data.location.href))
                    .execute(conn)
                    .chain_err(|| "Failed to update repo")?,
            None =>
                diesel::insert_into(repos)
                    .values((
                        uri.eq(repo_uri),
                        primary_db.eq(&primary_db_data.location.href)))
                    .execute(conn)
                    .chain_err(|| "Failed to insert repo")?,
        };
        Ok(())
    })
}

fn fetch_repomd_data(client: &http::Client, repo_uri: &str, data: &repomd::Data)
                     -> Box<Future<Item=PathBuf, Error=Error> + Send> {
    let decoder = Decoder::new(&data.location.href);
    let path = decoder.path().to_owned();
    let open_checksum = try_future!(data.open_checksum
        .as_ref()
        .ok_or_else(|| "Missing <open-checksum>".into()));
    if let Ok(hexdigest) = hashes::hexdigest_path(&path, &open_checksum.tpe) {
        if hexdigest == open_checksum.hexdigest {
            return Box::new(ok(path));
        }
    };
    let uri_str = repo_uri.to_owned() + "/" + &data.location.href;
    let uri = try_future!(uri_str.parse().chain_err(|| format!("Malformed URI: {}", uri_str)));
    let file = try_future!(create_file_all(&path));
    Box::new(http::checked_fetch(client, &uri)
        .and_then(move |response| { decoder.decode_response(file, response) })
        .and_then(|()| ok(path)))
}

fn bootstrap() -> Box<Future<Item=(), Error=Error> + Send> {
    dotenv().ok();
    let matches = App::new(env!("CARGO_PKG_NAME"))
        .version(env!("CARGO_PKG_VERSION"))
        .author(env!("CARGO_PKG_AUTHORS"))
        .about(env!("CARGO_PKG_DESCRIPTION"))
        .arg(Arg::with_name("DATABASE_URL")
            .long("database-url")
            .takes_value(true)
            .global(true))
        .arg(Arg::with_name("URI")
            .required(true)
            .index(1))
        .get_matches();
    let repo_uri = matches.value_of("URI").unwrap();
    let database_url = matches
        .value_of("DATABASE_URL")
        .map(|x| x.to_owned())
        .or_else(|| { env::var("DATABASE_URL").ok() })
        .unwrap_or_else(|| "index.sqlite".to_owned());
    let conn = try_future!(SqliteConnection::establish(&database_url)
        .chain_err(|| format!("SqliteConnection::establish({}) failed", database_url)));
    let repomd_uri_str = repo_uri.to_owned() + "/repodata/repomd.xml";
    let repomd_uri = try_future!(repomd_uri_str.parse()
        .chain_err(|| format!("Malformed URI: {}", repomd_uri_str)));
    let client = try_future!(http::make_client());
    Box::new(fetch_repomd(&client, &repomd_uri)
        .and_then({
            let repo_uri = repo_uri.to_owned();
            move |doc| -> Box<Future<Item=PathBuf, Error=Error> + Send> {
                let primary_db_data = try_future!(get_primary_db(&doc)
                    .ok_or_else(|| r#"Missing <data type="primary_db">"#.into()));
                try_future!(persist_repomd(&conn, &repo_uri, &primary_db_data));
                fetch_repomd_data(&client, &repo_uri, &primary_db_data)
            }
        })
        .and_then(|path| {
            let database_url = "file:".to_owned() +
                try_future!(path.to_str()
                    .ok_or_else(|| format!("Malformed path: {:?}", path).into())) +
                "?mode=ro";
            let _conn = try_future!(SqliteConnection::establish(&database_url)
                .chain_err(|| format!(
                    "SqliteConnection::establish({}) failed", database_url)));
            Box::new(ok(()))
        }))
}

fn main() {
    rt::run(Box::new(bootstrap().map_err(|e| {
        eprintln!("{}", e.display_chain());
        std::process::exit(1);
    })));
}
