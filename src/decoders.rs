use std::fs::File;
use std::io::Write;
use std::path::{Path, PathBuf};

use failure::{Error, ResultExt};
use futures::future::result;
use futures::Stream;
use hyper::{Body, Chunk, Response};
use hyper::rt::Future;

use crate::errors::{FutureExt, StreamExt};

pub trait Decoder {
    fn path(&self) -> &Path;
    fn decode_response(&self, file: File, response: Response<Body>)
                       -> Box<Future<Item=(), Error=Error> + Send>;
}

impl Decoder {
    pub fn from_href(href: &str) -> Box<Decoder + Send> {
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
            .context("Failed to read a chunk")
            .map_err(Error::from)
            .for_each(move |chunk| {
                result(file.write_all(&chunk))
                    .context("Failed to write a chunk")
                    .map_err(Error::from)
            }))
    }
}

struct XzDecoder {
    path: PathBuf,
}

fn decode_chunk(file: &mut File, xz: &mut xz2::stream::Stream, chunk: &Chunk) -> Result<(), Error> {
    let end = xz.total_in() as usize + chunk.len();
    let mut buf = Vec::with_capacity(8192);
    while (xz.total_in() as usize) < end {
        let remaining = end - xz.total_in() as usize;
        let remaining_bytes = &chunk[chunk.len() - remaining..chunk.len()];
        buf.clear();
        xz.process_vec(remaining_bytes, &mut buf, xz2::stream::Action::Run)
            .context("Failed to decompress a chunk")?;
        file.write_all(&buf)
            .context("Failed to write a chunk")?;
    }
    Ok(())
}

impl Decoder for XzDecoder {
    fn path(&self) -> &Path {
        &self.path
    }

    fn decode_response(&self, mut file: File, response: Response<Body>)
                       -> Box<Future<Item=(), Error=Error> + Send> {
        Box::new(result(xz2::stream::Stream::new_stream_decoder(std::u64::MAX, 0))
            .context("Failed to create an xz2::stream::Stream")
            .map_err(Error::from)
            .and_then(|mut xz| response
                .into_body()
                .context("Failed to read a chunk")
                .map_err(Error::from)
                .for_each(move |chunk| result(decode_chunk(&mut file, &mut xz, &chunk)))))
    }
}
