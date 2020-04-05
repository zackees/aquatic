use std::net::SocketAddr;
use std::sync::atomic::Ordering;
use std::time::Instant;
use std::vec::Drain;

use rand::{Rng, SeedableRng, rngs::SmallRng, thread_rng};
use rand::seq::IteratorRandom;

use bittorrent_udp::types::*;

use crate::common::*;


pub fn handle_connect_requests(
    state: &State,
    responses: &mut Vec<(Response, SocketAddr)>,
    requests: Drain<(ConnectRequest, SocketAddr)>,
){
    let now = Time(Instant::now());
    let mut rng = thread_rng();

    responses.extend(requests.map(|(request, src)| {
        let connection_id = ConnectionId(rng.gen());

        let key = ConnectionKey {
            connection_id,
            socket_addr: src,
        };

        state.connections.insert(key, now);

        let response = Response::Connect(
            ConnectResponse {
                connection_id,
                transaction_id: request.transaction_id,
            }
        );
        
        (response, src)
    }));
}


pub fn handle_announce_requests(
    state: &State,
    responses: &mut Vec<(Response, SocketAddr)>,
    requests: Drain<(AnnounceRequest, SocketAddr)>,
){
    responses.extend(requests.filter_map(|(request, src)| {
        let connection_key = ConnectionKey {
            connection_id: request.connection_id,
            socket_addr: src,
        };

        if !state.connections.contains_key(&connection_key){
            return None;
        }

        let mut torrent_data = state.torrents
            .entry(request.info_hash)
            .or_insert_with(|| TorrentData::default());

        let peer_key = PeerMapKey {
            ip: src.ip(),
            peer_id: request.peer_id,
        };

        let peer = Peer::from_announce_and_ip(&request, src.ip());
        let peer_status = peer.status;
        
        let opt_removed_peer = if peer.status == PeerStatus::Stopped {
            torrent_data.peers.remove(&peer_key)
        } else {
            torrent_data.peers.insert(peer_key, peer)
        };
        
        match peer_status {
            PeerStatus::Leeching => {
                torrent_data.num_leechers.fetch_add(1, Ordering::SeqCst);
            },
            PeerStatus::Seeding => {
                torrent_data.num_seeders.fetch_add(1, Ordering::SeqCst);
            },
            PeerStatus::Stopped => {}
        };

        if let Some(removed_peer) = opt_removed_peer {
            match removed_peer.status {
                PeerStatus::Leeching => {
                    torrent_data.num_leechers.fetch_sub(1, Ordering::SeqCst);
                },
                PeerStatus::Seeding => {
                    torrent_data.num_seeders.fetch_sub(1, Ordering::SeqCst);
                },
                PeerStatus::Stopped => {}
            }
        }

        let response_peers = extract_response_peers(&torrent_data.peers, 255); // FIXME num peers

        let response = Response::Announce(AnnounceResponse {
            transaction_id: request.transaction_id,
            announce_interval: AnnounceInterval(600), // FIXME
            leechers: NumberOfPeers(torrent_data.num_leechers.load(Ordering::SeqCst) as i32),
            seeders: NumberOfPeers(torrent_data.num_seeders.load(Ordering::SeqCst) as i32),
            peers: response_peers
        });

        Some((response, src))
    }));
}


pub fn handle_scrape_requests(
    state: &State,
    responses: &mut Vec<(Response, SocketAddr)>,
    requests: Drain<(ScrapeRequest, SocketAddr)>,
){
    let empty_stats = create_torrent_scrape_statistics(0, 0);

    responses.extend(requests.filter_map(|(request, src)| {
        let connection_key = ConnectionKey {
            connection_id: request.connection_id,
            socket_addr: src,
        };

        if !state.connections.contains_key(&connection_key){
            return None;
        }
        let mut stats: Vec<TorrentScrapeStatistics> = Vec::with_capacity(256);

        for info_hash in request.info_hashes.iter() {
            if let Some(torrent_data) = state.torrents.get(info_hash){
                stats.push(create_torrent_scrape_statistics(
                    torrent_data.num_seeders.load(Ordering::SeqCst) as i32,
                    torrent_data.num_leechers.load(Ordering::SeqCst) as i32,
                ));
            } else {
                stats.push(empty_stats);
            }
        }

        let response = Response::Scrape(ScrapeResponse {
            transaction_id: request.transaction_id,
            torrent_stats: stats,
        });

        Some((response, src))
    }));
}


/// Extract response peers
/// 
/// Do a random selection of peers if there are more than
/// `number_of_peers_to_take`. I tried out just selecting a random range,
/// but this might cause issues with the announcing peer getting back too
/// homogenous peers (based on when they were inserted into the map.)
/// 
/// Don't care if we send back announcing peer.
pub fn extract_response_peers(
    peer_map:                &PeerMap,
    number_of_peers_to_take: usize,
) -> Vec<ResponsePeer> {
    let peer_map_len = peer_map.len();

    if peer_map_len <= number_of_peers_to_take {
        peer_map.values()
            .map(Peer::to_response_peer)
            .collect()
    } else {
        let mut rng = SmallRng::from_rng(thread_rng()).unwrap();

        peer_map.values()
            .map(Peer::to_response_peer)
            .choose_multiple(&mut rng, number_of_peers_to_take)
    }
}


pub fn create_torrent_scrape_statistics(
    seeders: i32,
    leechers: i32
) -> TorrentScrapeStatistics {
    TorrentScrapeStatistics {
        seeders: NumberOfPeers(seeders),
        completed: NumberOfDownloads(0), // No implementation planned
        leechers: NumberOfPeers(leechers)
    }
}


#[cfg(test)]
mod tests {
    use std::time::Instant;
    use std::net::IpAddr;

    use indexmap::IndexMap;
    use quickcheck::{TestResult, quickcheck};

    use super::*;

    fn gen_peer_map_key_and_value(i: u8) -> (PeerMapKey, Peer) {
        let ip_address: IpAddr = "127.0.0.1".parse().unwrap();
        let peer_id = PeerId([i; 20]);

        let key = PeerMapKey {
            ip: ip_address, 
            peer_id,
        };
        let value = Peer {
            connection_id: ConnectionId(0),
            ip_address,
            id: peer_id,
            port: Port(1),
            status: PeerStatus::Leeching,
            last_announce: Time(Instant::now()),
        };

        (key, value)
    }

    #[test]
    fn test_extract_response_peers(){
        fn prop(data: (u8, u8)) -> TestResult {
            let gen_num_peers = data.0;
            let req_num_peers = data.1 as usize;

            let mut peer_map: PeerMap = IndexMap::new();

            for i in 0..gen_num_peers {
                let (key, value) = gen_peer_map_key_and_value(i);

                peer_map.insert(key, value);
            }

            let num_returned = extract_response_peers(
                &peer_map,
                req_num_peers
            ).len();

            let mut success = num_returned <= req_num_peers;

            if req_num_peers >= gen_num_peers as usize {
                success &= num_returned == gen_num_peers as usize;
            }

            TestResult::from_bool(success)
        }   

        quickcheck(prop as fn((u8, u8)) -> TestResult);
    }
}