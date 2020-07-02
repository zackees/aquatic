use std::net::IpAddr;
use hashbrown::HashMap;
use serde::{Serialize, Deserialize, Serializer};

use crate::common::Peer;

mod serde_helpers;

use serde_helpers::*;


pub fn serialize_response_peers_compact<S>(
    response_peers: &Vec<ResponsePeer>,
    serializer: S
) -> Result<S::Ok, S::Error> where S: Serializer {
    let mut bytes = Vec::with_capacity(response_peers.len() * 6);

    for peer in response_peers {
        match peer.ip_address {
            IpAddr::V4(ip) => {
                bytes.extend_from_slice(&u32::from(ip).to_be_bytes());
                bytes.extend_from_slice(&peer.port.to_be_bytes())
            },
            IpAddr::V6(_) => {
                continue
            }
        }
    }

    let text: String = bytes.into_iter().map(|byte| byte as char).collect();

    serializer.serialize_str(&text)
}


#[derive(Debug, Clone, Hash, PartialEq, Eq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct PeerId(
    #[serde(
        deserialize_with = "deserialize_20_char_string",
    )]
    pub String
);


#[derive(Debug, Clone, Hash, PartialEq, Eq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct InfoHash(
    #[serde(
        deserialize_with = "deserialize_20_char_string",
    )]
    pub String
);


#[derive(Clone, Copy, Debug, Serialize)]
pub struct ResponsePeer {
    pub ip_address: IpAddr,
    pub port: u16
}


impl ResponsePeer {
    pub fn from_peer(peer: &Peer) -> Self {
        let ip_address = peer.connection_meta.peer_addr.ip();

        Self {
            ip_address,
            port: peer.port
        }
    }
}


#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum AnnounceEvent {
    Started,
    Stopped,
    Completed,
    Empty
}


impl Default for AnnounceEvent {
    fn default() -> Self {
        Self::Empty
    }
}


#[derive(Debug, Clone, Deserialize)]
pub struct AnnounceRequest {
    pub info_hash: InfoHash,
    pub peer_id: PeerId,
    pub port: u16,
    #[serde(rename = "left")]
    pub bytes_left: usize,
    #[serde(default)]
    pub event: AnnounceEvent,
    /// FIXME: number: 0 or 1 to bool
    pub compact: u8,
    /// Requested number of peers to return
    pub numwant: usize,
}


#[derive(Debug, Clone, Serialize)]
pub struct AnnounceResponseSuccess {
    #[serde(rename = "interval")]
    pub announce_interval: usize,
    pub tracker_id: String, // Optional??
    pub complete: usize,
    pub incomplete: usize,
    #[serde(
        serialize_with = "serialize_response_peers_compact"
    )]
    pub peers: Vec<ResponsePeer>,
}


#[derive(Debug, Clone, Serialize)]
pub struct AnnounceResponseFailure {
    pub failure_reason: String,
}


#[derive(Debug, Clone, Deserialize)]
pub struct ScrapeRequest {
    #[serde(
        rename = "info_hash",
        deserialize_with = "deserialize_info_hashes" // FIXME: does this work?
    )]
    pub info_hashes: Vec<InfoHash>,
}


#[derive(Debug, Clone, Serialize)]
pub struct ScrapeStatistics {
    pub complete: usize,
    pub incomplete: usize,
    pub downloaded: usize,
}


#[derive(Debug, Clone, Serialize)]
pub struct ScrapeResponse {
    pub files: HashMap<InfoHash, ScrapeStatistics>,
}


#[derive(Debug, Clone, Deserialize)]
pub enum Request {
    Announce(AnnounceRequest),
    Scrape(ScrapeRequest),
}


impl Request {
    pub fn from_http(http: httparse::Request) -> Option<Self> {
        log::debug!("path: {:?}", http.path);

        let path = http.path?;

        let mut split_parts= path.splitn(2, '?');

        let path = split_parts.next()?;
        let query_string = split_parts.next()?;

        if path == "/announce" {
            let result: Result<AnnounceRequest, serde_urlencoded::de::Error> = serde_urlencoded::from_str(query_string);

            if let Err(ref err) = result {
                log::debug!("error: {}", err);
            }

            result.ok().map(Request::Announce)
        } else {
            let result: Result<ScrapeRequest, serde_urlencoded::de::Error> = serde_urlencoded::from_str(query_string);

            if let Err(ref err) = result {
                log::debug!("error: {}", err);
            }

            result.ok().map(Request::Scrape)
        }
    }
}


#[derive(Debug, Clone, Serialize)]
#[serde(untagged)]
pub enum Response {
    AnnounceSuccess(AnnounceResponseSuccess),
    AnnounceFailure(AnnounceResponseFailure),
    Scrape(ScrapeResponse)
}


impl Response {
    pub fn to_bytes(self) -> Vec<u8> {
        bendy::serde::to_bytes(&self).unwrap()
    }
}