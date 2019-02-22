use std::fs::File;
use std::io::Write;
use std::path::{Path, PathBuf};

use futures::future::{failed, ok};
use futures::Stream;
use hyper::{Body, Response};
use hyper::rt::Future;

use errors::*;

pub trait Decoder {
    fn path(&self) -> &Path;
    fn decode_response(&self, file: File, response: Response<Body>)
                       -> Box<Future<Item=(), Error=Error> + Send>;
}

impl Decoder {
    pub fn new(href: &str) -> Box<Decoder + Send> {
        if href.ends_with(".xz") {
            Box::new(XzDecoder { path: PathBuf::from(&href[0..href.len() - 3]) })
        } else {
            Box::new(PlainDecoder { path: PathBuf::from(href) })
        }
    }
}

struct PlainDecoder {
    path: PathBuf,
}

impl Decoder for PlainDecoder {
    fn path(&self) -> &Path {
        &self.path
    }

    fn decode_response(&self, mut file: File, response: Response<Body>)
                       -> Box<Future<Item=(), Error=Error> + Send> {
        Box::new(response
            .into_body()
            .map_err(|e| Error::with_chain(e, "Failed to read a chunk"))
            .for_each(move |chunk| {
                try_future!(file.write_all(&chunk)
                    .chain_err(|| "Failed to write a chunk"));
                Box::new(ok(()))
            }))
    }
}

struct XzDecoder {
    path: PathBuf,
}

impl Decoder for XzDecoder {
    fn path(&self) -> &Path {
        &self.path
    }

    fn decode_response(&self, mut file: File, response: Response<Body>)
                       -> Box<Future<Item=(), Error=Error> + Send> {
        let body = response
            .into_body()
            .map_err(|e| Error::with_chain(e, "Failed to read a chunk"));
        let mut xz = try_future!(xz2::stream::Stream::new_stream_decoder(std::u64::MAX, 0)
            .chain_err(|| "Failed to create an xz2::stream::Stream"));
        Box::new(body.for_each(move |chunk| {
            let end = xz.total_in() as usize + chunk.len();
            let mut buf = Vec::with_capacity(8192);
            while (xz.total_in() as usize) < end {
                let remaining = end - xz.total_in() as usize;
                let remaining_bytes = &chunk[chunk.len() - remaining..chunk.len()];
                buf.clear();
                try_future!(xz.process_vec(remaining_bytes, &mut buf, xz2::stream::Action::Run)
                    .chain_err(|| "Failed to decompress a chunk"));
                try_future!(file.write_all(&buf)
                    .chain_err(|| "Failed to write a chunk"));
            }
            Box::new(ok(()))
        }))
    }
}
