use std::collections::HashMap;
use std::str::from_utf8;

use arrayref::array_ref;
use failure::{bail, Error, format_err, ResultExt};
use futures::{Future, Stream};
use futures::future::result;
use nom::{be_u16, be_u32, be_u8, do_parse, named, tag, take};
use tokio_io::AsyncRead;
use tokio_io::io::read_exact;

use crate::errors::FutureExt;

pub struct Lead {
    pub magic: [u8; 4],
    pub major: u8,
    pub minor: u8,
    pub tpe: u16,
    pub archnum: u16,
    pub name: [u8; 66],
    pub osnum: u16,
    pub signature_type: u16,
    pub reserved: [u8; 16],
}

static LEAD_MAGIC: [u8; 4] = [0xed, 0xab, 0xee, 0xdb];

static LEAD_SIZE: usize = 96;

named!(parse_lead<Lead>,
    do_parse!(
        tag!(LEAD_MAGIC.as_ref()) >>
        major: be_u8 >>
        minor: be_u8 >>
        tpe: be_u16 >>
        archnum: be_u16 >>
        name: take!(66) >>
        osnum: be_u16 >>
        signature_type: be_u16 >>
        reserved: take!(16) >>
        (Lead {
            magic: LEAD_MAGIC,
            major,
            minor,
            tpe,
            archnum,
            name: *array_ref![name, 0, 66],
            osnum,
            signature_type,
            reserved: *array_ref![reserved, 0, 16],
        }))
);

pub fn read_lead<A: AsyncRead + Send + 'static>(
    a: A, pos: usize,
) -> impl Future<Item=(A, usize, Lead), Error=Error> {
    read_exact(a, vec![0u8; LEAD_SIZE])
        .context("Could not read RPM lead")
        .map_err(Error::from)
        .and_then(move |(a, buf): (A, Vec<u8>)| result(parse_lead(&buf)
            .map(|(_, lead)| lead)
            .map_err(|_| format_err!("Could not parse RPM lead - bad magic?")))
            .map(move |lead| (a, pos + LEAD_SIZE, lead)))
}

static HEADER_MAGIC: [u8; 3] = [0x8e, 0xad, 0xe8];

static HEADER_SIZE: usize = 16;

pub struct Header {
    pub magic: [u8; 3],
    pub version: u8,
    pub reserved: [u8; 4],
    pub index_entry_count: u32,
    pub store_size: u32,
}

named!(parse_header<Header>,
    do_parse!(
        tag!(HEADER_MAGIC.as_ref()) >>
        version: be_u8 >>
        reserved: take!(4) >>
        index_entry_count: be_u32 >>
        store_size: be_u32 >>
        (Header {
            magic: HEADER_MAGIC,
            version,
            reserved: *array_ref![reserved, 0, 4],
            index_entry_count,
            store_size,
        }))
);

pub fn read_header<A: AsyncRead + Send + 'static>(
    a: A, pos: usize,
) -> impl Future<Item=(A, usize, Header), Error=Error> {
    let padding = ((pos + 7) & !7) - pos;
    read_exact(a, vec![0u8; padding])
        .context("Could not pad RPM header")
        .map_err(Error::from)
        .and_then(move |(a, _)| read_exact(a, vec![0u8; HEADER_SIZE])
            .context("Could not read RPM header")
            .map_err(Error::from))
        .and_then(move |(a, buf): (A, Vec<u8>)| result(parse_header(&buf)
            .map(|(_, header)| header)
            .map_err(|_| format_err!("Could not parse RPM header - bad magic?")))
            .map(move |header| (a, pos + padding + HEADER_SIZE, header)))
}

pub struct IndexEntry {
    pub tag: u32,
    pub tpe: u32,
    pub offset: u32,
    pub count: u32,
}

static INDEX_ENTRY_SIZE: usize = 16;

named!(parse_index_entry<IndexEntry>,
    do_parse!(
        tag: be_u32 >>
        tpe: be_u32 >>
        offset: be_u32 >>
        count: be_u32 >>
        (IndexEntry {
            tag,
            tpe,
            offset,
            count,
        }))
);

pub fn read_index_entry<A: AsyncRead + Send + 'static>(
    a: A, pos: usize,
) -> impl Future<Item=(A, usize, IndexEntry), Error=Error> {
    read_exact(a, vec![0u8; INDEX_ENTRY_SIZE])
        .context("Could not read RPM index entry")
        .map_err(Error::from)
        .and_then(move |(a, buf): (A, Vec<u8>)| result(parse_index_entry(&buf)
            .map(|(_, index_entry)| index_entry)
            .map_err(|_| format_err!("Could not parse RPM index entry")))
            .map(move |index_entry| (a, pos + INDEX_ENTRY_SIZE, index_entry)))
}

pub struct FullHeader {
    pub header: Header,
    pub index_entries: HashMap<u32, IndexEntry>,
    pub store: Vec<u8>,
}

impl FullHeader {
    pub fn get_string_tag(&self, tag: u32, default: &str) -> Result<String, Error> {
        let entry = match self.index_entries.get(&tag) {
            Some(t) => t,
            None => return Ok(default.to_owned()),
        };
        if entry.tpe != 6 {
            bail!("RPM index entry has incorrect type");
        }
        if entry.offset as usize >= self.store.len() {
            bail!("RPM index entry points past the end of the store");
        }
        from_utf8(&self.store[entry.offset as usize..self.store.len()]
            .iter()
            .cloned()
            .take_while(|b| *b != 0)
            .collect::<Vec<_>>())
            .context("RPM index entry points to malformed UTF-8")
            .map_err(Error::from)
            .map(std::borrow::ToOwned::to_owned)
    }
}

pub fn read_full_header<A: AsyncRead + Send + 'static>(
    a: A, pos: usize,
) -> impl Future<Item=(A, usize, FullHeader), Error=Error> {
    read_header(a, pos).and_then(|(a, pos, header)| {
        let index_entries = HashMap::with_capacity(header.index_entry_count as usize);
        futures::stream::iter_ok(0..header.index_entry_count)
            .fold(((a, pos), index_entries), |((a, pos), mut index_entries), _| {
                read_index_entry(a, pos).map(|(a, pos, index_entry)| {
                    index_entries.insert(index_entry.tag, index_entry);
                    ((a, pos), index_entries)
                })
            })
            .map(|((a, pos), index_entries)| (a, pos, header, index_entries))
    }).and_then(|(a, pos, header, index_entries)|
        read_exact(a, vec![0u8; header.store_size as usize])
            .context("Could not read RPM store")
            .map_err(Error::from)
            .map(move |(a, store)| (
                a, pos + header.store_size as usize, FullHeader { header, index_entries, store })))
}
