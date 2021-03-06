#![feature(async_await, await_macro, futures_api)]

#[macro_use]
extern crate diesel;

#[macro_use]
pub mod async_await;
#[macro_use]
pub mod errors;

pub mod clap;
pub mod db;
pub mod cpio;
pub mod decoders;
pub mod fs;
pub mod hashes;
pub mod http;
pub mod metrics;
pub mod models;
pub mod repomd;
pub mod rpm;
pub mod schema;
pub mod sync;
pub mod tokio;
