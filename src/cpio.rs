use std::cmp::min;
use std::str::from_utf8;
use std::u64;

use failure::{Error, format_err, ResultExt};
use nom::{apply, do_parse, error_position, named, tag, take};
use tokio_io::AsyncRead;
use tokio_io::io::{read_exact, Window};

use crate::errors::FutureExt;

fn parse_u64(i: &[u8], n: usize) -> nom::IResult<&[u8], u64> {
    do_parse!(i, b: take!(n) >> (b))
        .and_then(|(i, b)| from_utf8(b)
            .map_err(|_| nom::Err::Error(error_position!(i, nom::ErrorKind::HexDigit)))
            .and_then(|s| u64::from_str_radix(s, 16)
                .map_err(|_| nom::Err::Error(error_position!(i, nom::ErrorKind::HexDigit)))
                .map(|v| (i, v))))
}

pub struct Header {
    pub c_magic: [u8; 6],
    pub c_ino: u64,
    pub c_mode: u64,
    pub c_uid: u64,
    pub c_gid: u64,
    pub c_nlink: u64,
    pub c_mtime: u64,
    pub c_filesize: u64,
    pub c_devmajor: u64,
    pub c_devminor: u64,
    pub c_rdevmajor: u64,
    pub c_rdevminor: u64,
    pub c_namesize: u64,
    pub c_checksum: u64,
}

static HEADER_MAGIC: [u8; 6] = [0x30, 0x37, 0x30, 0x37, 0x30, 0x31];

static HEADER_SIZE: usize = 110;

named!(parse_header<Header>,
    do_parse!(
        tag!(HEADER_MAGIC) >>
        c_ino: apply!(parse_u64, 8) >>
        c_mode: apply!(parse_u64, 8) >>
        c_uid: apply!(parse_u64, 8) >>
        c_gid: apply!(parse_u64, 8) >>
        c_nlink: apply!(parse_u64, 8) >>
        c_mtime: apply!(parse_u64, 8) >>
        c_filesize: apply!(parse_u64, 8) >>
        c_devmajor: apply!(parse_u64, 8) >>
        c_devminor: apply!(parse_u64, 8) >>
        c_rdevmajor: apply!(parse_u64, 8) >>
        c_rdevminor: apply!(parse_u64, 8) >>
        c_namesize: apply!(parse_u64, 8) >>
        c_checksum: apply!(parse_u64, 8) >>
        (Header {
            c_magic: HEADER_MAGIC,
            c_ino,
            c_mode,
            c_uid,
            c_gid,
            c_nlink,
            c_mtime,
            c_filesize,
            c_devmajor,
            c_devminor,
            c_rdevmajor,
            c_rdevminor,
            c_namesize,
            c_checksum,
        }))
);

pub async fn read_header<A: AsyncRead + Send + 'static>(
    a: A, pos: usize,
) -> Result<(A, usize, Header), Error> {
    let (a, buf) = await_old!(read_exact(a, vec![0u8; HEADER_SIZE])
        .context("Could not read CPIO header"))?;
    let (_, header) = parse_header(&buf)
        .map_err(|_| format_err!("Could not parse CPIO header - bad magic?"))?;
    Ok((a, pos + HEADER_SIZE, header))
}

pub async fn read_name<A: AsyncRead + Send + 'static>(
    a: A, pos: usize, size: usize,
) -> Result<(A, usize, String), Error> {
    let end = pos + size;
    let padding = ((end + 3) & !3) - end;
    let (a, name) = await_old!(read_exact(a, vec![0u8; size + padding])
        .context("Could not read CPIO file name"))?;
    let s = from_utf8(&name[..size - 1])
        .context("Malformed CPIO file name")?
        .to_owned();
    Ok((a, pos + size + padding, s))
}

pub struct Entry {
    pub header: Header,
    pub name: String,
    pub peek: Window<Vec<u8>>,
}

pub async fn read_entry_start<A: AsyncRead + Send + 'static>(
    a: A, pos: usize,
) -> Result<(A, usize, Option<Entry>), Error> {
    let (a, pos, header) = await!(read_header(a, pos))?;
    let c_namesize = header.c_namesize as usize;
    let (a, pos, name) = await!(read_name(a, pos, c_namesize))?;
    if name == "TRAILER!!!" {
        return Ok((a, pos, None));
    }
    let size = min(header.c_filesize as usize, 8192);
    let (a, peek) = await_old!(read_exact(a, Window::new(vec![0u8; size])))?;
    Ok((a, pos + size, Some(Entry {
        header,
        name,
        peek,
    })))
}

pub async fn read_entry_data<A: AsyncRead + Send + 'static>(
    a: A, pos: usize, c_filesize: u64, peek: Window<Vec<u8>>,
) -> Result<(A, usize, Vec<u8>), Error> {
    let c_filesize = c_filesize as usize;
    let mut data = vec![0u8; c_filesize];
    let peek_len = peek.end() - peek.start();
    data[..peek_len].copy_from_slice(peek.as_ref());
    let mut window = Window::new(data);
    window.set_start(peek_len);
    let (a, window) = await_old!(read_exact(a, window))?;
    Ok((a, pos + (c_filesize - peek_len), window.into_inner()))
}

pub async fn skip_entry_data<A: AsyncRead + Send + 'static>(
    mut a: A, mut pos: usize, c_filesize: u64, mut peek: Window<Vec<u8>>,
) -> Result<(A, usize), Error> {
    let mut remaining = c_filesize as usize - (peek.end() - peek.start());
    while remaining > 0 {
        if remaining < peek.end() {
            peek.set_end(remaining);
        }
        let (local_a, local_peek, n) = await_old!(tokio_io::io::read(a, peek))?;
        remaining -= n;
        pos += n;
        a = local_a;
        peek = local_peek;
    }
    Ok((a, pos))
}

pub async fn read_entry_end<A: AsyncRead + Send + 'static>(
    a: A, pos: usize,
) -> Result<(A, usize), Error> {
    let padding = ((pos + 3) & !3) - pos;
    let (a, _) = await_old!(tokio_io::io::read_exact(a, vec![0u8; padding]))?;
    Ok((a, pos + padding))
}
