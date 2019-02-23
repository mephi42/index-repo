use futures::Future;
use futures::future::ok;
use nom::{be_u16, be_u32, be_u8};
use tokio_io::AsyncRead;
use tokio_io::io::read_exact;

use errors::*;

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
            magic: LEAD_MAGIC.clone(),
            major,
            minor,
            tpe,
            archnum,
            name: {
                let mut tmp = [0u8; 66];
                tmp.copy_from_slice(&name[0..66]);
                tmp
            },
            osnum,
            signature_type,
            reserved: {
                let mut tmp = [0u8; 16];
                tmp.copy_from_slice(&reserved[0..16]);
                tmp
            },
        }))
);

type ReadLead<A> = Box<Future<Item=(A, Lead), Error=Error> + Send>;

pub fn read_lead<A: AsyncRead + Send + 'static>(a: A) -> ReadLead<A> {
    Box::new(read_exact(a, vec![0u8; 96])
        .chain_err(|| "Could not read RpmLead")
        .and_then(|(a, buf): (A, Vec<u8>)| -> ReadLead<A> {
            let (_, rpm_lead) = try_future!(parse_lead(&buf)
                .map_err(|_| "Could not parse RpmLead - bad magic?".into()));
            Box::new(ok((a, rpm_lead)))
        }))
}

static HEADER_MAGIC: [u8; 3] = [0x8e, 0xad, 0xe8];

pub struct Header {
    pub magic: [u8; 3],
    pub version: u8,
    pub reserved: [u8; 4],
    pub index_entry_count: u32,
    pub length: u32,
}

named!(parse_header<Header>,
    do_parse!(
        tag!(HEADER_MAGIC.as_ref()) >>
        version: be_u8 >>
        reserved: take!(4) >>
        index_entry_count: be_u32 >>
        length: be_u32 >>
        (Header {
            magic: HEADER_MAGIC.clone(),
            version,
            reserved: {
                let mut tmp = [0u8; 4];
                tmp.copy_from_slice(&reserved[0..4]);
                tmp
            },
            index_entry_count,
            length,
        }))
);

type ReadHeader<A> = Box<Future<Item=(A, Header), Error=Error> + Send>;

pub fn read_header<A: AsyncRead + Send + 'static>(a: A) -> ReadHeader<A> {
    Box::new(read_exact(a, vec![0u8; 16])
        .chain_err(|| "Could not read RpmHeader")
        .and_then(|(a, buf): (A, Vec<u8>)| -> ReadHeader<A> {
            let (_, rpm_header) = try_future!(parse_header(&buf)
                .map_err(|_| "Could not parse RpmHeader - bad magic?".into()));
            Box::new(ok((a, rpm_header)))
        }))
}

pub struct IndexEntry {
    pub tag: u32,
    pub tpe: u32,
    pub offset: u32,
    pub count: u32,
}

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

type ReadIndexEntry<A> = Box<Future<Item=(A, IndexEntry), Error=Error> + Send>;

pub fn read_index_entry<A: AsyncRead + Send + 'static>(a: A) -> ReadIndexEntry<A> {
    Box::new(read_exact(a, vec![0u8; 16])
        .chain_err(|| "Could not read ReadRpmIndexEntry")
        .and_then(|(a, buf): (A, Vec<u8>)| -> ReadIndexEntry<A> {
            let (_, rpm_header) = try_future!(parse_index_entry(&buf)
                .map_err(|_| "Could not parse ReadRpmIndexEntry".into()));
            Box::new(ok((a, rpm_header)))
        }))
}
