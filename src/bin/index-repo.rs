extern crate bytes;
extern crate clap;
extern crate diesel;
extern crate dotenv;
extern crate error_chain;
extern crate futures;
extern crate hyper;
extern crate hyper_tls;
extern crate index_repo;
extern crate tempfile;
extern crate xz2;

use std::env;
use std::fs::{create_dir_all, File};

use bytes::buf::Buf;
use clap::{App, Arg};
use diesel::prelude::*;
use dotenv::dotenv;
use error_chain::ChainedError;
use futures::future::{done, failed, ok};
use futures::Stream;
use hyper::rt;
use hyper::rt::Future;
use hyper_tls::HttpsConnector;

use index_repo::decoders::Decoder;
use index_repo::errors::*;
use index_repo::hashes;
use index_repo::models::Repo;
use index_repo::repomd;

type Client = hyper::Client<
    HttpsConnector<hyper::client::connect::HttpConnector>, hyper::body::Body>;

fn checked_fetch(client: &Client, uri: &hyper::Uri)
                 -> impl Future<Item=hyper::Response<hyper::Body>, Error=Error> {
    client.get(uri.clone())
        .chain_err({
            let uri = uri.clone();
            move || format!("Failed to fetch {}", uri)
        })
        .and_then({
            let uri = uri.clone();
            move |response| {
                let status = response.status();
                if status.is_success() {
                    Box::new(ok(response))
                } else {
                    Box::new(failed(format!(
                        "Failed to fetch {}: status-code {}", uri, status).into()))
                }
            }
        })
}

fn fetch_repomd(client: &Client, repomd_uri: &hyper::Uri)
                -> impl Future<Item=repomd::Document, Error=Error> {
    checked_fetch(client, repomd_uri)
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

fn fetch_repomd_data(client: &Client, repo_uri: &str, data: &repomd::Data)
                     -> Box<Future<Item=(), Error=Error> + Send> {
    let decoder = Decoder::new(&data.location.href);
    let open_checksum = match &data.open_checksum {
        Some(t) => t,
        None => return Box::new(failed("Missing <open-checksum>".into())),
    };
    if let Ok(hexdigest) = hashes::hexdigest_path(decoder.path(), &open_checksum.tpe) {
        if hexdigest == open_checksum.hexdigest {
            return Box::new(ok(()));
        }
    };
    let uri_str = repo_uri.to_owned() + "/" + &data.location.href;
    let uri = match uri_str.parse().chain_err(|| format!("Malformed URI: {}", uri_str)) {
        Ok(t) => t,
        Err(e) => return Box::new(failed(e)),
    };
    let file = {
        let parent_path = match decoder.path().parent() {
            Some(t) => t,
            None => return Box::new(failed(format!(
                "{:?} has no parent", decoder.path()).into()))
        };
        match create_dir_all(parent_path)
            .chain_err(|| format!("create_dir_all({:?}) failed", parent_path)) {
            Ok(()) => {}
            Err(e) => return Box::new(failed(e)),
        };
        match File::create(decoder.path())
            .chain_err(|| format!("File::create({:?}) failed", decoder.path())) {
            Ok(t) => t,
            Err(e) => return Box::new(failed(e)),
        }
    };
    Box::new(checked_fetch(client, &uri).and_then(move |response| {
        decoder.decode_response(file, response)
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
    let conn = match SqliteConnection::establish(&database_url)
        .chain_err(|| "SqliteConnection::establish() failed") {
        Ok(t) => t,
        Err(e) => return Box::new(failed(e)),
    };
    let repomd_uri_str = repo_uri.to_owned() + "/repodata/repomd.xml";
    let repomd_uri = match repomd_uri_str.parse()
        .chain_err(|| format!("Malformed URI: {}", repomd_uri_str)) {
        Ok(t) => t,
        Err(e) => return Box::new(failed(e)),
    };
    let https = match HttpsConnector::new(4)
        .chain_err(|| "HttpsConnector::new() failed") {
        Ok(t) => t,
        Err(e) => return Box::new(failed(e)),
    };
    let client = hyper::Client::builder().build::<_, hyper::Body>(https);
    Box::new(fetch_repomd(&client, &repomd_uri).and_then({
        let repo_uri = repo_uri.to_owned();
        move |doc| -> Box<Future<Item=(), Error=Error> + Send> {
            let primary_db_data = match get_primary_db(&doc) {
                Some(t) => t,
                None => return Box::new(failed(r#"Missing <data type="primary_db">"#.into())),
            };
            if let Err(e) = persist_repomd(&conn, &repo_uri, &primary_db_data) {
                return Box::new(failed(e));
            }
            fetch_repomd_data(&client, &repo_uri, &primary_db_data)
        }
    }))
}

fn main() {
    rt::run(Box::new(bootstrap().map_err(|e| {
        eprintln!("{}", e.display_chain());
        std::process::exit(1);
    })));
}
