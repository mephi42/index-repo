use std::fs::File;
use std::io::Read;
use std::path::Path;

use failure::{bail, Error, ResultExt};
use sha2::{Digest, Sha256};

trait Hash {
    fn update(&mut self, buf: &[u8]);
    fn hexdigest(self) -> String;
}

impl<T> Hash for T where T: Digest {
    fn update(&mut self, buf: &[u8]) {
        self.input(buf);
    }

    fn hexdigest(self) -> String {
        hex::encode(self.result())
    }
}

pub fn hexdigest_path(path: &Path, hash_type: &str) -> Result<String, Error> {
    let file = File::open(path).with_context(|_| format!("File::open({:?}) failed", path))?;
    hexdigest_file(file, hash_type)
}

fn hexdigest_file_1<H>(mut file: File, mut hash: H) -> Result<String, Error> where H: Hash {
    let mut buf = [0 as u8; 8192];
    loop {
        let n = file.read(&mut buf).context("File::read() failed")?;
        if n == 0 {
            break Ok(hash.hexdigest());
        }
        hash.update(&buf[0..n]);
    }
}

fn hexdigest_file(file: File, hash_type: &str) -> Result<String, Error> {
    match hash_type {
        "sha256" => hexdigest_file_1(file, Sha256::new()),
        _ => bail!("Unsupported hash type: {}", hash_type),
    }
}
