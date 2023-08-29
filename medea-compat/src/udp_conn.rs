use std::collections::HashMap;
use std::net::{SocketAddr, UdpSocket};

use futures::channel::mpsc;
use str0m::net::Transmit;

struct PeerConnection;

impl PeerConnection {
    pub fn new(peer_id: PeerId, tx: mpsc::UnboundedSender<EngineCommand>) -> mpsc::UnboundedReceiver<EngineEvent> {
        todo!()
    }
}

enum EngineEvent {
}

enum EngineCommand {
    RegisterPeer(PeerConnection),
}

struct PeerId(u32);

pub struct PeerConnectionEngine {
    last_peer_id: PeerId,
    connecting_peers: HashMap<PeerId, mpsc::UnboundedSender<EngineEvent>>,
    event_tx: HashMap<SocketAddr, mpsc::UnboundedSender<EngineEvent>>,
    cmd_rx: mpsc::UnboundedReceiver<EngineCommand>,
    cmd_tx: mpsc::UnboundedSender<EngineCommand>,
}

impl PeerConnectionEngine {
    pub fn new() -> Self {
        let (cmd_tx, cmd_rx) = mpsc::unbounded();
        Self {
            last_peer_id: PeerId(0),
            connecting_peers: Vec::new(),
            event_tx: HashMap::new(),
            cmd_tx,
            cmd_rx,
        }
    }

    pub fn create_peer(&mut self) -> Self {
        self.last_peer_id += 1;
        let peer = PeerConnection::new(self.last_peer_id, self.cmd_tx.clone());
        self.connecting_peers.insert(self.last_peer_id, peer);
    }

    fn on_peer_connected(&mut self, peer_id: PeerId, addr: SocketAddr) {
        if let Some(peer) = self.connecting_peers.remove(peer_id) {
            self.event_tx.insert(addr, peer);
        }
    }
}
