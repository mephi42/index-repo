#![feature(async_await, await_macro, futures_api)]

#[macro_use]
extern crate index_repo;

use clap::{app_from_crate, Arg, crate_authors, crate_description, crate_name, crate_version};
use failure::Error;
use tokio_io::io::{read, Window, write_all};

use index_repo::rpm;

async fn bootstrap() -> Result<(), Error> {
    let matches = app_from_crate!()
        .arg(Arg::with_name("RPM")
            .required(true)
            .index(1))
        .get_matches();
    let path = matches.value_of("RPM").unwrap();
    let in_file = await_old!(tokio::fs::File::open(path.to_owned()))?;
    let mut out_file = await_old!(tokio::fs::File::create(path.to_owned() + ".cpio"))?;
    let (mut a, _pos, _lead, _signature_header, _header) = await!(rpm::read_all_headers(in_file))?;
    let mut buf = vec![0u8; 8192];
    loop {
        let (local_a, local_buf, n) = await_old!(read(a, buf))?;
        if n == 0 {
            break;
        }
        let mut local_window = Window::new(local_buf);
        local_window.set_end(n);
        let (local_out_file, local_window) = await_old!(write_all(out_file, local_window))?;
        a = local_a;
        buf = local_window.into_inner();
        out_file = local_out_file;
    }
    Ok(())
}

fn main() -> Result<(), Error> {
    env_logger::init();
    index_repo::tokio::main(tokio_async_await::compat::backward::Compat::new(bootstrap()))
}
