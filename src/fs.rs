use std::fs::{create_dir_all, File};
use std::path::Path;

use failure::{Error, ResultExt};

pub fn create_file_all(path: &Path) -> Result<File, Error> {
    let parent_path = match path.parent() {
        Some(t) => t,
        None => bail!("{:?} has no parent", path),
    };
    create_dir_all(parent_path)
        .with_context(|_| format!("create_dir_all({:?}) failed", parent_path))?;
    File::create(path)
        .with_context(|_| format!("File::create({:?}) failed", path))
        .map_err(Error::from)
}
