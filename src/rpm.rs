use std::collections::HashMap;
use std::str::from_utf8;

use arrayref::array_ref;
use failure::{bail, Error, format_err, ResultExt};
use nom::{be_u16, be_u32, be_u8, do_parse, named, tag, take};
use tokio_io::AsyncRead;
use tokio_io::io::read_exact;
use xz2::read::XzDecoder;

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

pub async fn read_lead<A: AsyncRead + Send + 'static>(
    a: A, pos: usize,
) -> Result<(A, usize, Lead), Error> {
    let (a, buf) = await_old!(read_exact(a, vec![0u8; LEAD_SIZE])
        .context("Could not read RPM lead"))?;
    let (_, lead) = parse_lead(&buf)
        .map_err(|_| format_err!("Could not parse RPM lead - bad magic?"))?;
    Ok((a, pos + LEAD_SIZE, lead))
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

pub async fn read_header<A: AsyncRead + Send + 'static>(
    a: A, pos: usize,
) -> Result<(A, usize, Header), Error> {
    let padding = ((pos + 7) & !7) - pos;
    let (a, _) = await_old!(read_exact(a, vec![0u8; padding])
        .context("Could not pad RPM header"))?;
    let (a, buf) = await_old!(read_exact(a, vec![0u8; HEADER_SIZE])
            .context("Could not read RPM header"))?;
    let (_, header) = parse_header(&buf)
        .map_err(|_| format_err!("Could not parse RPM header - bad magic?"))?;
    Ok((a, pos + padding + HEADER_SIZE, header))
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

pub async fn read_index_entry<A: AsyncRead + Send + 'static>(
    a: A, pos: usize,
) -> Result<(A, usize, IndexEntry), Error> {
    let (a, buf) = await_old!(read_exact(a, vec![0u8; INDEX_ENTRY_SIZE])
        .context("Could not read RPM index entry"))?;
    let (_, index_entry) = parse_index_entry(&buf)
        .map_err(|_| format_err!("Could not parse RPM index entry"))?;
    Ok((a, pos + INDEX_ENTRY_SIZE, index_entry))
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

pub async fn read_full_header<A: AsyncRead + Send + 'static>(
    a: A, pos: usize,
) -> Result<(A, usize, FullHeader), Error> {
    let (mut a, mut pos, header) = await!(read_header(a, pos))?;
    let mut index_entries = HashMap::with_capacity(header.index_entry_count as usize);
    for _ in 0..header.index_entry_count {
        let (local_a, local_pos, index_entry) = await!(read_index_entry(a, pos))?;
        index_entries.insert(index_entry.tag, index_entry);
        a = local_a;
        pos = local_pos;
    }
    let (a, store) = await_old!(read_exact(a, vec![0u8; header.store_size as usize])
        .context("Could not read RPM store"))?;
    Ok((a, pos + header.store_size as usize, FullHeader { header, index_entries, store }))
}

pub async fn read_all_headers<A: AsyncRead + Send + 'static>(
    a: A,
) -> Result<(Box<AsyncRead + Send + 'static>, usize, Lead, FullHeader, FullHeader), Error> {
    let (a, pos, lead) = await!(read_lead(a, 0))?;
    let (a, pos, signature_header) = await!(read_full_header(a, pos))?;
    let (a, pos, header) = await!(read_full_header(a, pos))?;
    let format = header.get_string_tag(1124, "cpio")?;
    if format != "cpio" {
        bail!("Unsupported RPM payload format");
    }
    let coding = header.get_string_tag(1125, "gzip")?;
    let a: Box<AsyncRead + Send + 'static> = match coding.as_ref() {
        "xz" => Box::new(XzDecoder::new(a)),
        _ => bail!("Unsupported RPM payload coding"),
    };
    Ok((a, pos, lead, signature_header, header))
}
