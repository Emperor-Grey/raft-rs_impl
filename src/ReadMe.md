# Raft with FROST-Based Leader Election

## Overview

This project is a custom Raft consensus implementation with FROST (Flexible Round-Optimized Schnorr Threshold) signatures for cryptographic leader election and verification. It extends traditional Raft consensus by integrating threshold signatures, ensuring secure and verifiable leader election processes.

## Features

- **Raft Consensus:** Implements a custom Raft algorithm for leader election, log replication, and fault tolerance.
- **FROST Integration:** Uses FROST signatures for secure leader election and message verification.
- **Networking Layer:** Peer-to-peer communication using `socket2` for persistent connections.
- **Logging and Debugging:** Uses `simplelog` for structured logging.
- **Cryptographic Enhancements:** Ensures integrity and authenticity of leader elections using FROST threshold signatures.

## Dependencies

```toml
[dependencies]
serde = { version = "1.0.218", features = ["derive"] }
bincode = "1.3.3"
log = "0.4.26"
simplelog = "0.12.2"
rand = "0.8"
socket2 = "0.5.8"
serde_json = "1.0.139"
frost-ed25519 = "2.1.0"
```

## Installation

1. Clone the repository:
   ```sh
   git clone https://github.com/lohit-dev/raft-rs_impl
   cd raft-rs_impl
   ```
2. Build the project:
   ```sh
   cargo build --release
   ```
3. Run the nodes:
   ```sh
   cargo run
   ```

## Usage

- Nodes communicate using `socket2` and follow the Raft consensus model.
- Leader election is verified using FROST threshold signatures.
- Logs include messages like:
  - **Follower logs:**
    `{ "Heartbeat": { "term": 1, "peer_id": "server_4" } }`
  - **Leader logs:** `{ "HeartbeatLeaderEntry": { "term": 1, "peer_id": "server_4", "leader": true } }`

## Example Configuration

To run a multi-node setup, modify the configuration file or adjust command-line parameters to define multiple peers.

## Logging

This project uses `simplelog` for structured logging. You can configure logging levels by modifying the logger initialization in the source code.

## Reference

This project has taken reference from [`rsraft`](https://github.com/laurocaetano/rsraft) but extends it with FROST for cryptographic security.
