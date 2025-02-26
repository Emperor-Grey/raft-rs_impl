#![allow(unused)]

use std::net::{Ipv4Addr, SocketAddrV4};
use std::thread;
use std::time::{Duration, Instant};

use frost_ed25519::keys::SecretShare;
use frost_ed25519::{self as frost, Identifier};
use log::info;
use raft::network::Network;
use raft::node::Node;
use raft::types::{NodeConfig, Peer};
use rand::thread_rng;
use rand::Rng;
use simplelog::{ColorChoice, Config, LevelFilter, TermLogger, TerminalMode};
use std::collections::BTreeMap;

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
    let mut rng = rand::thread_rng();
    let node_configs = [
        (1, rng.gen_range(1..10)),
        (2, rng.gen_range(1..10)),
        (3, rng.gen_range(1..10)),
        (4, rng.gen_range(1..10)),
        (5, rng.gen_range(1..10)),
        (6, rng.gen_range(1..10)),
        (7, rng.gen_range(1..10)),
    ];

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

    let max_signers = node_configs.len() as u16;
    info!("Max signers: {}", max_signers);
    let min_signers = (max_signers / 2) + 1;
    let (shares, pubkey_package) = frost::keys::generate_with_dealer(
        max_signers,
        min_signers,
        frost::keys::IdentifierList::Default,
        &mut rng,
    )
    .expect("Failed to generate keys...");

    // Node ID -> Secret Share
    // BTreeMap for same order or hashmap random shit (I tried that's why)
    let mut node_secret_shares: BTreeMap<u16, SecretShare> = BTreeMap::new();
    for (i, (_identifier, share)) in shares.iter().enumerate() {
        node_secret_shares.insert((i + 1) as u16, share.clone());
    }

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

        // Giving frost key to the node
        let secret_share = node_secret_shares.get(&(i as u16 + 1)).cloned();
        node.set_frost_key(secret_share);

        // Start the node
        node.start();

        nodes.push(node);
        networks.push(network);
    }

    // Test FROST signature verification
    let message = b"Test message for distributed signing";

    // Generate nonces and commitments for each participating node
    let mut nonces_map = BTreeMap::new();
    let mut commitments_map = BTreeMap::new();
    let mut rng = thread_rng();

    // Collect commitments from min_signers nodes
    for i in 1..=min_signers {
        let node = &nodes[(i - 1) as usize];
        if let Some(key_share) = &node.frost_key {
            let key_package = frost::keys::KeyPackage::try_from(key_share.clone())
                .expect("Failed to create key package");

            let (nonces, commitments) =
                frost::round1::commit(key_package.signing_share(), &mut rng);

            let identifier = i.try_into().expect("Invalid identifier");
            nonces_map.insert(identifier, nonces);
            commitments_map.insert(identifier, commitments);
        }
    }

    // Create signing package
    let signing_package = frost::SigningPackage::new(commitments_map, message);

    // Collect signature shares
    let mut signature_shares = BTreeMap::new();
    for (identifier, nonces) in nonces_map.iter() {
        // Create a mapping from identifier to node index
        // We'll use the fact that you created identifiers as i + 1 earlier
        for (node_idx, node) in nodes.iter().enumerate() {
            // Check if this node has the FROST key with matching identifier
            if let Some(key_share) = &node.frost_key {
                let key_package = frost::keys::KeyPackage::try_from(key_share.clone())
                    .expect("Failed to create key package");

                // Check if this node's identifier matches the current one
                if key_package.identifier() == identifier {
                    if let Ok(signature_share) =
                        node.sign_message(message, nonces.clone(), &signing_package)
                    {
                        signature_shares.insert(*identifier, signature_share);
                    }
                    break; // Found the matching node, no need to check others
                }
            }
        }
    }

    // Aggregate signatures
    if let Ok(group_signature) =
        frost::aggregate(&signing_package, &signature_shares, &pubkey_package)
    {
        // Verify the signature with each node
        for node in &nodes {
            let is_valid = node.verify_signature(message, &group_signature, &pubkey_package);
            info!(
                "Node {} signature verification result: {}",
                node.server.id, is_valid
            );
        }
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
