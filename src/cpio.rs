use std::fs::File;
use std::io::{Seek, Write};
use std::io::SeekFrom;
use std::str::from_utf8;
use std::u64;

use failure::{Error, format_err, ResultExt};
use nom::{apply, do_parse, error_position, named, tag, take};
use tempfile::tempfile;
use tokio_io::AsyncRead;
use tokio_io::io::read_exact;

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

pub async fn read_entry<A: AsyncRead + Send + 'static>(
    a: A, pos: usize,
) -> Result<(A, usize, Option<(Header, String, File)>), Error> {
    let (a, pos, header) = await!(read_header(a, pos))?;
    let c_namesize = header.c_namesize as usize;
    let (mut a, mut pos, name) = await!(read_name(a, pos, c_namesize))?;
    if name == "TRAILER!!!" {
        return Ok((a, pos, None));
    }
    let mut tmp = tempfile()?;
    let mut remaining = header.c_filesize as usize;
    let mut buf = vec![0u8; 8192];
    while remaining > 0 {
        if remaining < buf.len() {
            buf.truncate(remaining);
        }
        let (local_a, local_buf, n) = await_old!(tokio_io::io::read(a, buf))?;
        tmp.write_all(&local_buf[0..n])?;
        remaining -= n;
        pos += n;
        a = local_a;
        buf = local_buf;
    }
    tmp.seek(SeekFrom::Start(0))?;
    let padding = ((pos + 3) & !3) - pos;
    let (a, _) = await_old!(tokio_io::io::read_exact(a, vec![0u8; padding]))?;
    pos += padding;
    Ok((a, pos, Some((header, name, tmp))))
}
