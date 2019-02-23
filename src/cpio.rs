use std::str::from_utf8;
use std::u64;

use futures::Future;
use futures::future::ok;
use tokio_io::AsyncRead;
use tokio_io::io::read_exact;

use errors::*;

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

pub type ReadHeader<A> = Box<Future<Item=(A, usize, Header), Error=Error> + Send>;

pub fn read_header<A: AsyncRead + Send + 'static>(a: A, pos: usize) -> ReadHeader<A> {
    Box::new(read_exact(a, vec![0u8; HEADER_SIZE])
        .chain_err(|| "Could not read CPIO header")
        .and_then(move |(a, buf): (A, Vec<u8>)| -> ReadHeader<A> {
            let (_, header) = try_future!(parse_header(&buf)
                .map_err(|_| "Could not parse CPIO header - bad magic?".into()));
            Box::new(ok((a, pos + HEADER_SIZE, header)))
        }))
}
