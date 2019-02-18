extern crate bytes;
extern crate clap;
extern crate diesel;
extern crate dotenv;
extern crate error_chain;
extern crate futures;
extern crate hyper;
extern crate hyper_tls;
extern crate index_repo;

use std::env;

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

use index_repo::errors::*;
use index_repo::models::Repo;
use index_repo::repomd;
use index_repo::schema::repos::dsl::*;

type Client = hyper::Client<
    HttpsConnector<hyper::client::connect::HttpConnector>, hyper::body::Body>;

fn fetch_repomd(client: &Client, repomd_uri: hyper::Uri)
                -> Box<Future<Item=repomd::Document, Error=Error> + Send> {
    Box::new(client.get(repomd_uri.clone())
        .chain_err({
            let repomd_uri = repomd_uri.clone();
            move || format!("Failed to fetch {}", repomd_uri)
        })
        .and_then({
            let repomd_uri = repomd_uri.clone();
            move |response| {
                let status = response.status();
                if status.is_success() {
                    response.into_body().concat2().chain_err({
                        let repomd_uri = repomd_uri.clone();
                        move || format!(
                            "Failed to fetch {}: failed to read response body", repomd_uri)
                    })
                } else {
                    Box::new(failed(format!(
                        "Failed to fetch {}: status-code {}", repomd_uri, status).into()))
                }
            }
        })
        .and_then(|body| done(repomd::Document::parse(body.reader()))))
}

fn persist_repomd(conn: &SqliteConnection, repo_uri: &str, doc: &repomd::Document) -> Result<()> {
    match doc.data.iter().find(|data| data.tpe == "primary_db") {
        Some(data) => {
            conn.transaction(|| {
                let repo_vec = repos
                    .filter(uri.eq(repo_uri))
                    .limit(1)
                    .load::<Repo>(conn)
                    .chain_err(|| "Failed to query repo by uri")?;
                match repo_vec.first() {
                    Some(repo) =>
                        diesel::update(repos.filter(id.eq(repo.id)))
                            .set(primary_db.eq(&data.location.href))
                            .execute(conn)
                            .chain_err(|| "Failed to update repo")?,
                    None =>
                        diesel::insert_into(repos)
                            .values((
                                uri.eq(repo_uri),
                                primary_db.eq(&data.location.href)))
                            .execute(conn)
                            .chain_err(|| "Failed to insert repo")?,
                };
                Ok(())
            })
        }
        None => Err(r#"Missing <data type="primary_db">"#.into()),
    }
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
        .unwrap_or("index.sqlite".to_owned());
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
    Box::new(fetch_repomd(&client, repomd_uri).and_then({
        let repo_uri = repo_uri.to_owned();
        move |doc| {
            match persist_repomd(&conn, &repo_uri, &doc) {
                Ok(()) => ok(()),
                Err(e) => failed(e),
            }
        }
    }))
}

fn main() {
    rt::run(Box::new(bootstrap().map_err(|e| {
        eprintln!("{}", e.display_chain());
        std::process::exit(1);
    })));
}
