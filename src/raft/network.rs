use crate::raft::types::{Message, Peer};
use log::{error, info};
use socket2::{Domain, Protocol, SockAddr, Socket, Type};
use std::collections::HashMap;
use std::io::{Read, Write};
use std::net::Ipv4Addr;
use std::net::{SocketAddrV4, TcpListener, TcpStream};
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::Arc;
use std::sync::Mutex;
use std::thread;

pub struct Network {
    listen_addr: SocketAddrV4,
    message_tx: Sender<(Message, Peer)>,
    message_rx: Receiver<(Message, Peer)>,
    connections: Arc<Mutex<HashMap<SocketAddrV4, TcpStream>>>,
}

impl Network {
    pub fn new(listen_addr: SocketAddrV4) -> Self {
        let (message_tx, message_rx) = channel();
        let connections = Arc::new(Mutex::new(HashMap::new()));

        let network = Self {
            listen_addr,
            message_tx,
            message_rx,
            connections,
        };

        network.start_listener();
        network
    }

    fn start_listener(&self) {
        let listener_addr = self.listen_addr;
        let tx = self.message_tx.clone();
        let _connections = Arc::clone(&self.connections);

        thread::spawn(move || {
            let listener = TcpListener::bind(listener_addr).unwrap();
            info!("Started listening on {}", listener_addr);

            for stream in listener.incoming() {
                match stream {
                    Ok(stream) => {
                        let tx = tx.clone();
                        thread::spawn(move || {
                            handle_connection(stream, tx);
                        });
                    }
                    Err(e) => error!("Error accepting connection: {}", e),
                }
            }
        });
    }

    fn get_or_create_connection(&self, addr: SocketAddrV4) -> Result<TcpStream, std::io::Error> {
        let mut connections = self.connections.lock().unwrap();

        if addr == self.listen_addr {
            return Err(std::io::Error::new(
                std::io::ErrorKind::Other,
                "Cannot connect to self",
            ));
        }

        if let Some(stream) = connections.get(&addr) {
            if stream.peer_addr().is_ok() {
                if let Ok(stream_clone) = stream.try_clone() {
                    return Ok(stream_clone);
                }
            } else {
                connections.remove(&addr);
            }
        }

        info!("Creating new connection to {}", addr);

        let socket = Socket::new(Domain::IPV4, Type::STREAM, Some(Protocol::TCP))?;
        socket.set_nodelay(true)?;
        socket.set_keepalive(true)?;

        socket.connect(&SockAddr::from(addr))?;
        let tcp_stream: TcpStream = socket.into();

        let stream_clone = tcp_stream.try_clone()?;
        connections.insert(addr, tcp_stream);

        Ok(stream_clone)
    }

    pub fn receive(&self) -> Option<(Message, Peer)> {
        match self.message_rx.try_recv() {
            Ok(msg) => Some(msg),
            Err(_) => None,
        }
    }

    pub fn send(&self, message: Message, to: &Peer) {
        // Don't send messages to ourselves
        if to.address == self.listen_addr {
            return;
        }

        match self.get_or_create_connection(to.address) {
            Ok(mut stream) => {
                let serialized = bincode::serialize(&message).unwrap();
                match stream.write_all(&serialized) {
                    Ok(_) => {
                        // Ensure the message is sent
                        if let Err(e) = stream.flush() {
                            error!("Failed to flush connection to {}: {}", to.address, e);
                            self.connections.lock().unwrap().remove(&to.address);
                        }
                    }
                    Err(e) => {
                        error!("Failed to send message to {}: {}", to.address, e);
                        self.connections.lock().unwrap().remove(&to.address);
                    }
                }
            }
            Err(e) => {
                error!("Failed to connect to {}: {}", to.address, e);
            }
        }
    }

    pub fn broadcast(&self, message: Message, peers: &[Peer]) {
        for peer in peers {
            self.send(message.clone(), peer);
        }
    }
}

fn handle_connection(mut stream: TcpStream, tx: Sender<(Message, Peer)>) {
    let mut buffer = [0; 1024];
    loop {
        match stream.read(&mut buffer) {
            Ok(0) => {
                // Connection was closed by peer
                info!("Connection closed by peer");
                break;
            }
            Ok(n) => {
                if let Ok(message) = bincode::deserialize::<Message>(&buffer[..n]) {
                    // Extract port from the message itself
                    let port = match &message {
                        Message::RequestVote { port, .. } => *port,
                        Message::RequestVoteResponse { port, .. } => *port,
                        Message::Heartbeat { port, .. } => *port,
                    };
                    // info!("The port that we get is {}", port);

                    let peer = Peer {
                        id: format!("server_{}", (port - 55000 + 1)),
                        address: SocketAddrV4::new(Ipv4Addr::new(127, 0, 0, 1), port),
                    };

                    if let Err(e) = tx.send((message, peer)) {
                        error!("Failed to forward message: {}", e);
                        break;
                    }
                }
            }
            Err(e) => {
                error!("Failed to read from stream: {}", e);
                break;
            }
        }
    }
}
