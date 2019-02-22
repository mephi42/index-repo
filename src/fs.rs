use std::fs::{create_dir_all, File};
use std::path::Path;

use errors::*;

pub fn create_file_all(path: &Path) -> Result<File> {
    let parent_path = match path.parent() {
        Some(t) => t,
        None => return Err(format!("{:?} has no parent", path).into())
    };
    create_dir_all(parent_path)
        .chain_err(|| format!("create_dir_all({:?}) failed", parent_path))?;
    File::create(path)
        .chain_err(|| format!("File::create({:?}) failed", path))
}
