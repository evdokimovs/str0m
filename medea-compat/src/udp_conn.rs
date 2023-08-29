use std::collections::HashMap;
use std::net::{SocketAddr, UdpSocket};

use futures::channel::mpsc;
use str0m::net::Transmit;

struct PeerConnection;

impl PeerConnection {
    pub fn new(tx: mpsc::UnboundedSender<EngineCommand>) -> mpsc::UnboundedReceiver<EngineEvent> {
        todo!()
    }
}

enum EngineEvent {
}

enum EngineCommand {
    RegisterPeer(PeerConnection),
}

pub struct PeerConnectionEngine {
    connecting_peers: HashMap<String, mpsc::UnboundedSender<EngineEvent>>,
    event_tx: HashMap<SocketAddr, mpsc::UnboundedSender<EngineEvent>>,
    cmd_rx: mpsc::UnboundedReceiver<EngineCommand>,
    cmd_tx: mpsc::UnboundedSender<EngineCommand>,
}

impl PeerConnectionEngine {
    pub fn new() -> Self {
        let (cmd_tx, cmd_rx) = mpsc::unbounded();
        Self {
            connecting_peers: Vec::new(),
            event_tx: HashMap::new(),
            cmd_tx,
            cmd_rx,
        }
    }

    pub fn create_peer(&mut self) -> Self {
        let peer = PeerConnection::new(self.cmd_tx.clone());
        // self.connecting_peers.insert()
    }
}
