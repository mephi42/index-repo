use futures::Future;
use futures::future::ok;
use nom::{be_u16, be_u8};
use tokio_io::AsyncRead;
use tokio_io::io::read_exact;

use errors::*;

pub struct RpmLead {
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

static RPM_LEAD_MAGIC: [u8; 4] = [0xed, 0xab, 0xee, 0xdb];

named!(parse_rpm_lead<RpmLead>,
    do_parse!(
        tag!(RPM_LEAD_MAGIC.as_ref()) >>
        major: be_u8 >>
        minor: be_u8 >>
        tpe: be_u16 >>
        archnum: be_u16 >>
        name: take!(66) >>
        osnum: be_u16 >>
        signature_type: be_u16 >>
        reserved: take!(16) >>
        (RpmLead {
            magic: RPM_LEAD_MAGIC.clone(),
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
        })
  )
);

type ReadRpmLead<A> = Box<Future<Item=(A, RpmLead), Error=Error> + Send>;

pub fn read_rpm_lead<A: AsyncRead + Send + 'static>(a: A) -> ReadRpmLead<A> {
    Box::new(read_exact(a, vec![0u8; 96])
        .chain_err(|| "Could not read RpmLead")
        .and_then(|(a, buf): (A, Vec<u8>)| -> ReadRpmLead<A> {
            let (_, rpm_lead) = try_future!(parse_rpm_lead(&buf)
                .map_err(|_| "Could not parse RpmLead - bad magic?".into()));
            Box::new(ok((a, rpm_lead)))
        }))
}
