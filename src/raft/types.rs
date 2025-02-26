use log::info;
use serde::{Deserialize, Serialize};
use std::{
    net::SocketAddrV4,
    time::{Duration, Instant},
};

#[derive(Debug, Clone, PartialEq)]
pub enum NodeState {
    Follower,
    Candidate,
    Leader,
}

#[derive(Serialize, Deserialize, Debug)]
pub enum LogEntry {
    Heartbeat {
        term: u64,
        peer_id: String,
    },
    HeartbeatLeaderEntry {
        term: u64,
        peer_id: String,
        leader: bool,
    },
}

#[derive(Debug, Clone)]
pub struct Peer {
    pub id: String,
    pub address: SocketAddrV4,
}

#[derive(Debug)]
pub struct Leader {
    pub id: String,
    pub _term: u64,
}

#[derive(Debug)]
pub struct Server {
    pub id: String,
    pub address: SocketAddrV4,
    pub state: NodeState,
    pub term: u64,
    pub log_entries: Vec<LogEntry>,
    pub voted_for: Option<Peer>,
    pub next_timeout: Option<Instant>,
    pub timeout: Duration,
    pub current_leader: Option<Leader>,
    pub number_of_peers: usize,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct VoteRequest {
    pub term: u64,
    pub candidate_id: String,
}

pub struct VoteResponse {
    pub _term: u64,
    pub vote_granted: bool,
}

// Had to include port number i am getting random port numbers for each node otherwise
// and no proper connection if i don't do this
#[derive(Debug, Serialize, Deserialize, Clone)]
pub enum Message {
    RequestVote {
        term: u64,
        candidate_id: u64,
        port: u16,
    },
    RequestVoteResponse {
        term: u64,
        vote_granted: bool,
        _port: u16,
    },
    Heartbeat {
        term: u64,
        leader_id: u64,
        _port: u16,
    },
}

pub struct NodeConfig {
    pub id: u64,
    pub election_timeout: Duration,
    pub heartbeat_interval: Duration,
}

impl Default for NodeConfig {
    fn default() -> Self {
        Self {
            id: 1,
            election_timeout: Duration::from_millis(1500),
            heartbeat_interval: Duration::from_millis(500),
        }
    }
}

impl Server {
    pub fn _new(
        timeout: Duration,
        number_of_peers: usize,
        address: SocketAddrV4,
        id: String,
    ) -> Self {
        Server {
            id,
            state: NodeState::Follower,
            term: 0,
            log_entries: Vec::new(),
            voted_for: None,
            next_timeout: None,
            timeout,
            current_leader: None,
            number_of_peers,
            address,
        }
    }

    pub fn refresh_timeout(&mut self) {
        self.next_timeout = Some(Instant::now() + self.timeout);
    }

    pub fn become_leader(&mut self) {
        if self.state == NodeState::Candidate {
            info!(
                "Server {} has won the election! The new term is: {}",
                self.id, self.term
            );
            self.state = NodeState::Leader;
            self.next_timeout = None;
        }
    }

    pub fn _start(&mut self) {
        self.refresh_timeout();
    }

    pub fn has_timed_out(&mut self) -> bool {
        match self.next_timeout {
            Some(t) => Instant::now() > t,
            None => false,
        }
    }
}
