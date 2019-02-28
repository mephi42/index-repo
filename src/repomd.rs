use std::io::Read;

use failure::{Error, ResultExt, SyncFailure};
use serde_derive::Deserialize;
use serde_xml_rs::from_reader;

#[derive(Clone, Debug, Deserialize, PartialEq)]
pub struct Checksum {
    #[serde(rename = "type")]
    pub tpe: String,
    #[serde(rename = "$value")]
    pub hexdigest: String,
}

#[derive(Debug, Deserialize, PartialEq)]
pub struct Location {
    pub href: String,
}

#[derive(Debug, Deserialize, PartialEq)]
pub struct Data {
    #[serde(rename = "type")]
    pub tpe: String,
    pub checksum: Checksum,
    #[serde(rename = "open-checksum")]
    pub open_checksum: Option<Checksum>,
    pub location: Location,
    pub timestamp: i64,
    pub size: i64,
    #[serde(rename = "open-size")]
    pub open_size: Option<i64>,
}

#[derive(Debug, Deserialize, PartialEq)]
pub struct Document {
    pub revision: i64,
    pub data: Vec<Data>,
}

impl Document {
    pub fn parse<R: Read>(r: R) -> Result<Document, Error> {
        from_reader(r)
            .map_err(SyncFailure::new)
            .context("Malformed repomd.xml")
            .map_err(Error::from)
    }
}
