use hickory_proto::op::{Edns, Message, OpCode, Query, ResponseCode};
use hickory_proto::rr::rdata::opt::{EdnsCode, EdnsOption};
use hickory_proto::serialize::binary::{BinDecodable, BinDecoder};

pub const MAX_QUERY_BYTES: usize = 512;
pub const HEADER_BYTES: usize = 12;

const FLAG_RESPONSE: u8 = 0x80;
const FLAG_RECURSION_DESIRED: u8 = 0x01;
const OPCODE_SHIFT: u8 = 3;
const OPCODE_MASK: u8 = 0x0f;
const MAX_ADDITIONAL: u16 = 1;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Request {
    pub id: u16,
    pub recursion_desired: bool,
    pub checking_disabled: bool,
    pub query: Query,
    pub edns: Option<Edns>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Rejection {
    pub id: u16,
    pub op_code: OpCode,
    pub recursion_desired: bool,
    pub code: ResponseCode,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Parsed {
    Drop,
    Reject(Rejection),
    Query(Request),
}

impl Request {
    pub fn edns_version(&self) -> u8 {
        self.edns.as_ref().map_or(0, Edns::version)
    }

    pub fn dnssec_ok(&self) -> bool {
        self.edns
            .as_ref()
            .is_some_and(|edns| edns.flags().dnssec_ok)
    }

    pub fn client_subnet(&self) -> Option<String> {
        let edns = self.edns.as_ref()?;
        match edns.option(EdnsCode::Subnet)? {
            EdnsOption::Subnet(subnet) => {
                Some(format!("{}/{}", subnet.addr(), subnet.source_prefix()))
            }
            _ => None,
        }
    }
}

pub fn parse(bytes: &[u8]) -> Parsed {
    if bytes.len() < HEADER_BYTES || bytes.len() > MAX_QUERY_BYTES {
        return Parsed::Drop;
    }
    if bytes[2] & FLAG_RESPONSE != 0 {
        return Parsed::Drop;
    }
    let id = u16::from_be_bytes([bytes[0], bytes[1]]);
    let rejection = |op_code, code| {
        Parsed::Reject(Rejection {
            id,
            op_code,
            recursion_desired: bytes[2] & FLAG_RECURSION_DESIRED != 0,
            code,
        })
    };
    let op_code = OpCode::from_u8((bytes[2] >> OPCODE_SHIFT) & OPCODE_MASK);
    if op_code != OpCode::Query {
        return rejection(op_code, ResponseCode::NotImp);
    }
    let count = |offset: usize| u16::from_be_bytes([bytes[offset], bytes[offset + 1]]);
    if count(4) != 1 || count(6) != 0 || count(8) != 0 || count(10) > MAX_ADDITIONAL {
        return rejection(op_code, ResponseCode::FormErr);
    }
    let mut decoder = BinDecoder::new(bytes);
    let Ok(message) = Message::read(&mut decoder) else {
        return rejection(op_code, ResponseCode::FormErr);
    };
    if decoder.index() != bytes.len() {
        return rejection(op_code, ResponseCode::FormErr);
    }
    let Some(query) = message.queries.into_iter().next() else {
        return rejection(op_code, ResponseCode::FormErr);
    };
    Parsed::Query(Request {
        id,
        recursion_desired: message.metadata.recursion_desired,
        checking_disabled: message.metadata.checking_disabled,
        query,
        edns: message.edns,
    })
}
