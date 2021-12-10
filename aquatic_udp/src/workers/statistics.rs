use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{Duration, Instant};

use crate::common::*;
use crate::config::Config;

pub fn run_statistics_worker(config: Config, state: State) {
    let ipv4_active = config.network.address.is_ipv4() || !config.network.only_ipv6;
    let ipv6_active = config.network.address.is_ipv6();

    let mut last_ipv4 = Instant::now();
    let mut last_ipv6 = Instant::now();

    loop {
        ::std::thread::sleep(Duration::from_secs(config.statistics.interval));

        println!("General:");
        println!("  access list entries: {}", state.access_list.load().len());

        if ipv4_active {
            println!("IPv4:");
            gather_and_print_for_protocol(&config, &state.statistics_ipv4, &mut last_ipv4);
        }
        if ipv6_active {
            println!("IPv6:");
            gather_and_print_for_protocol(&config, &state.statistics_ipv6, &mut last_ipv6);
        }

        println!();
    }
}

fn gather_and_print_for_protocol(config: &Config, statistics: &Statistics, last: &mut Instant) {
    let requests_received: f64 = statistics.requests_received.fetch_and(0, Ordering::AcqRel) as f64;
    let responses_sent: f64 = statistics.responses_sent.fetch_and(0, Ordering::AcqRel) as f64;
    let bytes_received: f64 = statistics.bytes_received.fetch_and(0, Ordering::AcqRel) as f64;
    let bytes_sent: f64 = statistics.bytes_sent.fetch_and(0, Ordering::AcqRel) as f64;

    let now = Instant::now();

    let elapsed = (now - *last).as_secs_f64();

    *last = now;

    let requests_per_second = requests_received / elapsed;
    let responses_per_second: f64 = responses_sent / elapsed;
    let bytes_received_per_second: f64 = bytes_received / elapsed;
    let bytes_sent_per_second: f64 = bytes_sent / elapsed;

    let num_torrents: usize = sum_atomic_usizes(&statistics.torrents);
    let num_peers = sum_atomic_usizes(&statistics.peers);

    println!(
        "  requests/second: {:10.2}, responses/second: {:10.2}",
        requests_per_second, responses_per_second
    );

    println!(
        "  bandwidth: {:7.2} Mbit/s in, {:7.2} Mbit/s out",
        bytes_received_per_second * 8.0 / 1_000_000.0,
        bytes_sent_per_second * 8.0 / 1_000_000.0,
    );

    println!("  number of torrents: {}", num_torrents);
    println!(
        "  number of peers: {} (updated every {} seconds)",
        num_peers, config.cleaning.torrent_cleaning_interval
    );
}

fn sum_atomic_usizes(values: &[AtomicUsize]) -> usize {
    values.iter().map(|n| n.load(Ordering::Acquire)).sum()
}