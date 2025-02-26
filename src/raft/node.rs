use crate::raft::storage::Storage;
use crate::raft::types::{
    Leader, LogEntry, Message, NodeConfig, NodeState, Peer, Server, VoteResponse,
};
use frost_ed25519::keys::PublicKeyPackage;
use frost_ed25519::keys::SecretShare;
use frost_ed25519::{self as frost, Error, Signature};
use log::info;
use rand::Rng;
use std::{
    io,
    net::SocketAddrV4,
    time::{Duration, Instant},
};

pub struct Node {
    pub server: Server,
    pub peers: Vec<Peer>,
    pub config: NodeConfig,
    pub last_heartbeat: Option<Instant>,
    pub votes_received: u64,
    pub storage: Storage,
    pub frost_key: Option<SecretShare>,
}

impl Node {
    pub fn new(config: NodeConfig, address: SocketAddrV4, peers: Vec<Peer>) -> Self {
        let id = format!("server_{}", config.id);
        let timeout = Self::random_election_timeout(config.election_timeout);

        let server = Server {
            id: id.clone(),
            address,
            state: NodeState::Follower,
            term: 0,
            log_entries: Vec::new(),
            voted_for: None,
            next_timeout: None,
            timeout,
            current_leader: None,
            number_of_peers: peers.len(),
        };

        let storage = Storage::new(config.id).expect("Failed to initialize storage");

        Self {
            server,
            peers,
            config,
            last_heartbeat: None,
            votes_received: 0,
            storage,
            frost_key: None,
        }
    }

    fn random_election_timeout(base_timeout: Duration) -> Duration {
        let mut rng = rand::thread_rng();
        let random_ms = rng.gen_range(300..800);
        base_timeout + Duration::from_millis(random_ms)
    }

    pub fn start(&mut self) {
        info!("Starting server at: {}", self.server.address);
        self.server.refresh_timeout();
        info!(
            "The server {}, has a timeout of {} seconds.",
            self.server.id,
            self.server.timeout.as_secs()
        );
    }

    pub fn tick(&mut self) -> Vec<Message> {
        let mut messages = Vec::new();
        let port = self.server.address.port();

        match self.server.state {
            NodeState::Follower | NodeState::Candidate => {
                if self.server.has_timed_out() {
                    self.start_election();
                    messages.push(Message::RequestVote {
                        term: self.server.term,
                        candidate_id: self.config.id,
                        port,
                    });
                }
            }
            NodeState::Leader => {
                let now = Instant::now();
                if self.last_heartbeat.map_or(true, |time| {
                    now.duration_since(time) >= self.config.heartbeat_interval
                }) {
                    self.last_heartbeat = Some(now);

                    // Log leader's heartbeat
                    let _ = self.append_log_entry(LogEntry::Heartbeat {
                        term: self.server.term,
                        peer_id: self.server.id.clone(),
                    });

                    messages.push(Message::Heartbeat {
                        term: self.server.term,
                        leader_id: self.config.id,
                        _port: port,
                    });
                }
            }
        }

        messages
    }

    pub fn handle_message(&mut self, message: Message, from_peer: &Peer) -> Option<Message> {
        info!("Handling message from {}: {:?}\n", from_peer.id, message);
        match message {
            Message::RequestVote {
                term,
                candidate_id,
                port: _,
            } => {
                let response = self.handle_vote_request(term, candidate_id, from_peer);
                Some(Message::RequestVoteResponse {
                    term: self.server.term,
                    vote_granted: response.vote_granted,
                    _port: self.server.address.port(),
                })
            }
            Message::RequestVoteResponse {
                term,
                vote_granted,
                _port: _,
            } => {
                self.handle_vote_response(term, vote_granted, from_peer);
                None
            }
            Message::Heartbeat {
                term,
                leader_id,
                _port: _,
            } => {
                self.handle_heartbeat(term, leader_id, from_peer);
                None
            }
        }
    }

    fn start_election(&mut self) {
        self.server.term += 1;
        self.server.state = NodeState::Candidate;
        // Count our own vote
        self.votes_received = 1;

        // Vote for self
        self.server.voted_for = Some(Peer {
            id: self.server.id.clone(),
            address: self.server.address,
        });

        info!(
            "Server {}, with term {}, started the election process.",
            self.server.id, self.server.term
        );

        self.server.timeout = Self::random_election_timeout(self.config.election_timeout);
        self.server.refresh_timeout();
    }

    fn handle_vote_request(
        &mut self,
        term: u64,
        _candidate_id: u64,
        from_peer: &Peer,
    ) -> VoteResponse {
        // If the term is greater, update our term and become follower
        if term > self.server.term {
            self.server.term = term;
            self.become_follower(None);
            self.server.voted_for = None; // Reset vote when term changes
        }

        // Grant vote if:
        // 1. Candidate's term is >= our term
        // 2. We haven't voted for anyone else in this term
        let vote_granted = term >= self.server.term
            && (self.server.voted_for.is_none()
                || self
                    .server
                    .voted_for
                    .as_ref()
                    .map(|p| p.id == from_peer.id)
                    .unwrap_or(false));

        if vote_granted {
            self.server.voted_for = Some(from_peer.clone());
            self.server.refresh_timeout();
        }

        VoteResponse {
            _term: self.server.term,
            vote_granted,
        }
    }

    fn handle_vote_response(&mut self, term: u64, vote_granted: bool, _from_peer: &Peer) {
        if term > self.server.term {
            self.server.term = term;
            self.become_follower(None);
            return;
        }

        // Only count votes if we're still a candidate and in the same term
        if self.server.state == NodeState::Candidate && term == self.server.term && vote_granted {
            self.votes_received += 1;

            let total_nodes = self.server.number_of_peers + 1; // Total nodes including self
            let votes_needed = (total_nodes / 2) + 1; // Majority needed

            // If we have majority, become leader
            if self.votes_received >= votes_needed as u64 {
                self.server.become_leader();
                self.last_heartbeat = None; // Will trigger immediate heartbeat

                info!(
                    "Server {} becoming leader with {} votes out of {} needed",
                    self.server.id, self.votes_received, votes_needed
                );
            }
        }
    }

    fn handle_heartbeat(&mut self, term: u64, _leader_id: u64, from_peer: &Peer) {
        info!(
            "Server {} with term {}, received heartbeat from {} with term {}",
            self.server.id, self.server.term, from_peer.id, term
        );

        // If term is greater or equal, update term and become follower
        if term >= self.server.term {
            let _old_term = self.server.term;
            self.server.term = term;

            // Update leader information
            let leader = Leader {
                id: from_peer.id.clone(),
                _term: term,
            };

            // If we weren't already following this leader, log the change
            if self
                .server
                .current_leader
                .as_ref()
                .map_or(true, |l| l.id != leader.id)
            {
                info!(
                    "Server {} becoming follower. The new leader is: {}",
                    self.server.id, leader.id
                );
            }

            self.become_follower(Some(leader));

            // for the node_id
            let _ = self.append_log_entry(LogEntry::Heartbeat {
                term: term,
                peer_id: from_peer.id.clone(),
            });

            // Add heartbeat to log entries
            self.server.log_entries.push(LogEntry::Heartbeat {
                term,
                peer_id: from_peer.id.clone(),
            });
        }
    }

    fn become_follower(&mut self, leader: Option<Leader>) {
        self.server.state = NodeState::Follower;
        self.server.current_leader = leader;
        self.server.refresh_timeout();
    }

    pub fn append_log_entry(&mut self, entry: LogEntry) -> io::Result<()> {
        self.storage.append_entry(&entry)?;
        self.server.log_entries.push(entry);
        Ok(())
    }

    // From here it's frost shit
    pub fn set_frost_key(&mut self, secret_share: Option<SecretShare>) {
        self.frost_key = secret_share.clone();
        if secret_share.is_some() {
            info!(
                "FROST key set for node {} (server_{})",
                self.config.id, self.server.id
            );
        }
    }

    pub fn verify_signature(
        &self,
        message: &[u8],
        signature: &Signature,
        pubkey_package: &PublicKeyPackage,
    ) -> bool {
        match pubkey_package.verifying_key().verify(message, signature) {
            Ok(_) => {
                info!(
                    "Node {} successfully verified signature for message",
                    self.server.id
                );
                true
            }
            Err(e) => {
                info!("Node {} failed to verify signature: {}", self.server.id, e);
                false
            }
        }
    }

    pub fn sign_message(
        &self,
        message: &[u8],
        nonces: frost::round1::SigningNonces,
        signing_package: &frost::SigningPackage,
    ) -> Result<frost::round2::SignatureShare, frost::Error> {
        if let Some(key_package) = self
            .frost_key
            .as_ref()
            .map(|share| frost::keys::KeyPackage::try_from(share.clone()))
        {
            match key_package {
                Ok(package) => frost::round2::sign(signing_package, &nonces, &package),
                Err(e) => {
                    info!("Failed to create key package: {}", e);
                    Err(e)
                }
            }
        } else {
            info!("No FROST key available for signing");
            info!("Invalid signature share");
            Err(Error::InvalidSignature)
        }
    }
}
