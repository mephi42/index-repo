use failure::{Error, format_err, ResultExt};
use futures::future::{failed, ok};
use hyper::rt::Future;
use hyper_tls::HttpsConnector;

use crate::errors::FutureExt;

pub type Client = hyper::Client<
    HttpsConnector<hyper::client::connect::HttpConnector>, hyper::body::Body>;

pub fn make_client() -> Result<Client, Error> {
    let https = HttpsConnector::new(4)
        .context("HttpsConnector::new() failed")?;
    Ok(hyper::Client::builder().build::<_, hyper::Body>(https))
}

pub fn checked_fetch(client: &Client, uri: &hyper::Uri)
                     -> impl Future<Item=hyper::Response<hyper::Body>, Error=Error> {
    client.get(uri.clone())
        .with_context({
            let uri = uri.clone();
            move |_| format!("Failed to fetch {}", uri)
        })
        .map_err(Error::from)
        .and_then({
            let uri = uri.clone();
            move |response| {
                let status = response.status();
                if status.is_success() {
                    Box::new(ok(response))
                } else {
                    Box::new(failed(format_err!(
                        "Failed to fetch {}: status-code {}", uri, status)))
                }
            }
        })
}
