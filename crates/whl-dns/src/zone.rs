use std::net::{Ipv4Addr, Ipv6Addr};

use hickory_proto::op::{Edns, Message, MessageType, OpCode, ResponseCode};
use hickory_proto::rr::rdata::{A, AAAA, HINFO, NS, SOA, TXT};
use hickory_proto::rr::{Name, RData, Record, RecordType};
use whl_core::capture_id::CaptureId;
use whl_core::endpoint_id::EndpointId;

use crate::query::{HEADER_BYTES, Rejection, Request};

pub const NEGATIVE_TTL: u32 = 60;
pub const APEX_NS_TTL: u32 = 300;
pub const CAPTURED_TTL: u32 = 0;
pub const PLAIN_UDP_LIMIT: usize = 512;
pub const EDNS_UDP_LIMIT: usize = 1232;

const SOA_REFRESH: i32 = 3600;
const SOA_RETRY: i32 = 600;
const SOA_EXPIRE: i32 = 86_400;
const HINFO_CPU: &str = "RFC8482";

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Target {
    Outside,
    Apex,
    Captured { id: EndpointId, below: String },
    Unknown,
}

#[derive(Debug, Clone)]
pub struct Zone {
    name: Name,
    labels: Vec<Vec<u8>>,
    ns: Name,
    rname: Name,
    serial: u32,
    answer_a: Option<Ipv4Addr>,
    answer_aaaa: Option<Ipv6Addr>,
}

impl Zone {
    pub fn new(
        name: Name,
        ns: Name,
        rname: Name,
        serial: u32,
        answer_a: Option<Ipv4Addr>,
        answer_aaaa: Option<Ipv6Addr>,
    ) -> Self {
        let labels = name.iter().map(<[u8]>::to_vec).collect();
        Self {
            name,
            labels,
            ns,
            rname,
            serial,
            answer_a,
            answer_aaaa,
        }
    }

    pub fn classify(&self, qname: &Name) -> Target {
        let labels: Vec<&[u8]> = qname.iter().collect();
        let Some(split) = labels.len().checked_sub(self.labels.len()) else {
            return Target::Outside;
        };
        let inside = labels[split..]
            .iter()
            .zip(&self.labels)
            .all(|(asked, own)| asked.eq_ignore_ascii_case(own));
        if !inside {
            return Target::Outside;
        }
        let Some(id_index) = split.checked_sub(1) else {
            return Target::Apex;
        };
        let id = std::str::from_utf8(labels[id_index])
            .ok()
            .and_then(|text| EndpointId::parse(&text.to_ascii_lowercase()).ok());
        match id {
            Some(id) => Target::Captured {
                id,
                below: labels[..id_index]
                    .iter()
                    .map(|label| String::from_utf8_lossy(label).into_owned())
                    .collect::<Vec<_>>()
                    .join("."),
            },
            None => Target::Unknown,
        }
    }

    pub fn apex_answer(&self, request: &Request) -> Message {
        let mut message = base(request, ResponseCode::NoError, true);
        match request.query.query_type() {
            RecordType::SOA => {
                message.add_answer(self.soa_record());
            }
            RecordType::NS => {
                message.add_answer(Record::from_rdata(
                    self.name.clone(),
                    APEX_NS_TTL,
                    RData::NS(NS(self.ns.clone())),
                ));
            }
            RecordType::ANY => {
                message.add_answer(minimal_any(request, NEGATIVE_TTL));
            }
            _ => {
                message.add_authority(self.soa_record());
            }
        }
        message
    }

    pub fn nxdomain(&self, request: &Request) -> Message {
        let mut message = base(request, ResponseCode::NXDomain, true);
        message.add_authority(self.soa_record());
        message
    }

    pub fn captured_answer(&self, request: &Request, capture: Option<CaptureId>) -> Message {
        let mut message = base(request, ResponseCode::NoError, true);
        let owner = request.query.name().clone();
        match request.query.query_type() {
            RecordType::A => {
                if let Some(address) = self.answer_a {
                    message.add_answer(Record::from_rdata(
                        owner,
                        CAPTURED_TTL,
                        RData::A(A(address)),
                    ));
                }
            }
            RecordType::AAAA => {
                if let Some(address) = self.answer_aaaa {
                    message.add_answer(Record::from_rdata(
                        owner,
                        CAPTURED_TTL,
                        RData::AAAA(AAAA(address)),
                    ));
                }
            }
            RecordType::TXT => {
                if let Some(capture) = capture {
                    message.add_answer(Record::from_rdata(
                        owner,
                        CAPTURED_TTL,
                        RData::TXT(TXT::new(vec![capture.to_string()])),
                    ));
                }
            }
            RecordType::ANY => {
                message.add_answer(minimal_any(request, CAPTURED_TTL));
            }
            _ => {}
        }
        message
    }

    fn soa_record(&self) -> Record {
        Record::from_rdata(
            self.name.clone(),
            NEGATIVE_TTL,
            RData::SOA(SOA::new(
                self.ns.clone(),
                self.rname.clone(),
                self.serial,
                SOA_REFRESH,
                SOA_RETRY,
                SOA_EXPIRE,
                NEGATIVE_TTL,
            )),
        )
    }
}

fn minimal_any(request: &Request, ttl: u32) -> Record {
    Record::from_rdata(
        request.query.name().clone(),
        ttl,
        RData::HINFO(HINFO::new(HINFO_CPU.to_owned(), String::new())),
    )
}

fn response_edns() -> Edns {
    let mut edns = Edns::new();
    edns.set_max_payload(EDNS_UDP_LIMIT as u16);
    edns
}

fn base(request: &Request, code: ResponseCode, authoritative: bool) -> Message {
    let mut message = Message::new(request.id, MessageType::Response, OpCode::Query);
    message.metadata.authoritative = authoritative;
    message.metadata.recursion_desired = request.recursion_desired;
    message.metadata.checking_disabled = request.checking_disabled;
    message.metadata.response_code = code;
    message.add_query(request.query.clone());
    if request.edns.is_some() {
        message.set_edns(response_edns());
    }
    message
}

pub fn refusal(request: &Request, code: ResponseCode) -> Message {
    base(request, code, false)
}

pub fn truncated(request: &Request) -> Message {
    let mut message = base(request, ResponseCode::NoError, true);
    message.metadata.truncation = true;
    message
}

pub fn bad_version(request: &Request) -> Message {
    base(request, ResponseCode::BADVERS, false)
}

pub fn rejection_bytes(rejection: &Rejection) -> Vec<u8> {
    let mut flags_high = 0x80_u8 | (u8::from(rejection.op_code) << 3);
    if rejection.recursion_desired {
        flags_high |= 0x01;
    }
    let mut bytes = vec![0_u8; HEADER_BYTES];
    bytes[..2].copy_from_slice(&rejection.id.to_be_bytes());
    bytes[2] = flags_high;
    bytes[3] = rejection.code.low();
    bytes
}

pub fn udp_limit(request: &Request) -> usize {
    match &request.edns {
        Some(edns) => usize::from(edns.max_payload()).clamp(PLAIN_UDP_LIMIT, EDNS_UDP_LIMIT),
        None => PLAIN_UDP_LIMIT,
    }
}

pub fn encode(message: &Message) -> Option<Vec<u8>> {
    match message.to_vec() {
        Ok(bytes) => Some(bytes),
        Err(error) => {
            tracing::error!(%error, "a DNS response could not be encoded");
            None
        }
    }
}

pub fn encode_within(
    message: &Message,
    request: &Request,
    limit: Option<usize>,
) -> Option<Vec<u8>> {
    let bytes = encode(message)?;
    let Some(limit) = limit else {
        return Some(bytes);
    };
    if bytes.len() <= limit {
        return Some(bytes);
    }
    encode(&truncated(request))
}
