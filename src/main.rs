#![allow(unused)]

use std::net::{Ipv4Addr, SocketAddrV4};
use std::thread;
use std::time::{Duration, Instant};

use raft::network::Network;
use raft::node::Node;
use raft::types::{NodeConfig, Peer};
use simplelog::{ColorChoice, Config, LevelFilter, TermLogger, TerminalMode};

mod raft;

fn main() {
    // Initialize logger
    TermLogger::init(
        LevelFilter::Info,
        Config::default(),
        TerminalMode::Mixed,
        ColorChoice::Auto,
    )
    .unwrap();

    // Define our cluster configuration
    let node_configs = [(1, 5), (2, 3), (3, 6), (4, 4)];

    // Create socket addresses for each node
    let node_addresses: Vec<SocketAddrV4> = node_configs
        .iter()
        .enumerate()
        .map(|(i, _)| SocketAddrV4::new(Ipv4Addr::new(127, 0, 0, 1), 55000 + i as u16))
        .collect();

    // Create peer information
    let peers: Vec<Vec<Peer>> = node_addresses
        .iter()
        .enumerate()
        .map(|(i, _)| {
            node_addresses
                .iter()
                .enumerate()
                .filter(|(j, _)| i != *j)
                .map(|(j, addr)| Peer {
                    id: format!("server_{}", j + 1),
                    address: *addr,
                })
                .collect()
        })
        .collect();

    // Create and start all nodes
    let mut nodes = Vec::new();
    let mut networks = Vec::new();

    for (i, (node_id, timeout_secs)) in node_configs.iter().enumerate() {
        // Create node config
        let config = NodeConfig {
            id: *node_id,
            election_timeout: Duration::from_secs(*timeout_secs as u64),
            heartbeat_interval: Duration::from_millis(1000),
        };

        // Create node and network
        let mut node = Node::new(config, node_addresses[i], peers[i].clone());
        let network = Network::new(node_addresses[i]);

        // Start the node
        node.start();

        nodes.push(node);
        networks.push(network);
    }

    // Main event loop
    let mut last_tick = Instant::now();
    let tick_interval = Duration::from_millis(500); // Check state every 500ms
    let mut retry_count = 0;
    const MAX_RETRIES: u32 = 3;

    loop {
        thread::sleep(Duration::from_millis(10));

        let now = Instant::now();
        if now.duration_since(last_tick) >= tick_interval {
            last_tick = now;

            for (i, node) in nodes.iter_mut().enumerate() {
                // Handle incoming messages with retry
                while let Some((message, from_peer)) = networks[i].receive() {
                    match node.handle_message(message, &from_peer) {
                        Some(response) => {
                            retry_count = 0;
                            while retry_count < MAX_RETRIES {
                                let peer_to_respond = Peer {
                                    id: from_peer.id.clone(),
                                    address: from_peer.address,
                                };
                                networks[i].send(response.clone(), &peer_to_respond);
                                break;
                            }
                        }
                        None => retry_count = 0,
                    }
                }

                // Handle outgoing messages
                let outgoing_messages = node.tick();
                for message in outgoing_messages {
                    networks[i].broadcast(message, &node.peers);
                }
            }
        }
    }
}
