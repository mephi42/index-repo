use failure::{bail, Error, ResultExt};
use hyper_tls::HttpsConnector;
use log::info;

use crate::errors::FutureExt;

pub type Client = hyper::Client<
    HttpsConnector<hyper::client::connect::HttpConnector>, hyper::body::Body>;

pub fn make_client() -> Result<Client, Error> {
    let https = HttpsConnector::new(4)
        .context("HttpsConnector::new() failed")?;
    Ok(hyper::Client::builder().build::<_, hyper::Body>(https))
}

pub async fn checked_fetch(
    client: &Client, uri: hyper::Uri,
) -> Result<hyper::Response<hyper::Body>, Error> {
    info!("Fetching {}...", &uri);
    let response = await_old!(client.get(uri.clone())
        .with_context({
            let uri = uri.clone();
            move |_| format!("Failed to fetch {}", &uri)
        }))?;
    let status = response.status();
    if status.is_success() {
        Ok(response)
    } else {
        bail!("Failed to fetch {}: status-code {}", &uri, status);
    }
}
