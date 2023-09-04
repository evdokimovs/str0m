use std::collections::HashMap;
use std::io::ErrorKind;
use std::net::{IpAddr, SocketAddr, UdpSocket};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use tokio::sync::mpsc::{self, error::TryRecvError};
use crate::peer::PeerConnectionHandle;

use crate::util::proto::PeerId;
use crate::util::{
    peer::{PeerHandle},
    proto::{EngineCommand, EngineEvent},
};
use crate::peer::PeerConnection;

pub struct PeerConnectionEngine {
    id: Mutex<u32>,

    pub local_addr: SocketAddr,

    event_tx: HashMap<SocketAddr, mpsc::UnboundedSender<EngineEvent>>,

    cmd_tx: mpsc::UnboundedSender<EngineCommand>,
    loop_handle: std::thread::JoinHandle<()>,
}

impl PeerConnectionEngine {
    pub fn new(bind_addr: IpAddr) -> Self {
        let (cmd_tx, mut cmd_rx) = mpsc::unbounded_channel();
        let socket = UdpSocket::bind(format!("{bind_addr}:19305")).unwrap();
        let local_addr = socket.local_addr().unwrap();
        let mut peers_tx = HashMap::<SocketAddr, mpsc::UnboundedSender<EngineEvent>>::new();
        let mut connecting_peers = HashMap::<PeerId, mpsc::UnboundedSender<EngineEvent>>::new();

        let loop_handle = std::thread::spawn(move || loop {
            let mut read_buffer = vec![0; 2000];

            match cmd_rx.try_recv() {
                Ok(event) => match event {
                    EngineCommand::PeerCreated(id, peer_tx) => {
                        log::error!("PeerCreated");
                        connecting_peers.insert(id, peer_tx);
                    }
                    EngineCommand::PeerConnected(id, dst) => {
                        log::error!("PeerConnected");
                        let peer = connecting_peers.remove(&id).unwrap();
                        peers_tx.insert(dst, peer);
                    }
                    EngineCommand::Transmit(t) => {
                        socket.send_to(&t.contents, t.destination).unwrap();
                    }
                },
                Err(TryRecvError::Empty) => {}
                Err(TryRecvError::Disconnected) => {
                    log::error!("PEER DROP");
                    break;
                }
            }

            socket
                .set_read_timeout(Some(Duration::from_micros(10)))
                .expect("setting socket read timeout");

            let (data, src, at) = match socket.recv_from(&mut read_buffer) {
                Ok((n, source)) => (read_buffer[0..n].to_vec(), source, Instant::now()),

                Err(e) => match e.kind() {

                    ErrorKind::WouldBlock | ErrorKind::TimedOut => {
                        continue;
                    }
                    _ => panic!("UdpSocket read failed: {e:?}"),
                },
            };

            if let Some(mut peer_tx) = peers_tx.get(&src) {
                println!("Received Packet for Peer by SocketAddr");
                peer_tx
                    .send(EngineEvent::PacketReceived {
                        at,
                        payload: data,
                        source: src,
                        destination: local_addr,
                    })
                    .unwrap();
            } else {
                println!("Received Packet for Peer for all connecting Peers");
                for tx in connecting_peers.values_mut() {
                    tx.send(EngineEvent::PacketReceived {
                        at,
                        payload: data.clone(),
                        source: src,
                        destination: local_addr,
                    }).unwrap();
                }
                // if connecting_peers.len() == 1 {
                //     for tx in connecting_peers.values_mut() {
                //         tx.send(EngineEvent::PacketReceived {
                //             at,
                //             payload: data.clone(),
                //             source: src,
                //             destination: local_addr,
                //         }).unwrap();
                //     }
                // } else {
                //     connecting_peers.get(&PeerId(1)).unwrap().send(EngineEvent::PacketReceived {
                //         at,
                //         payload: data.clone(),
                //         source: src,
                //         destination: local_addr,
                //     }).unwrap();
                // }

            }
        });

        Self {
            id: Mutex::new(0),
            local_addr,
            event_tx: HashMap::new(),
            cmd_tx,
            loop_handle: loop_handle,
        }
    }

    pub fn create_peer(&self) -> PeerConnectionHandle {
        let mut id = self.id.lock().unwrap();
        *id += 1;

        let peer = PeerConnection::new(PeerId(*id), self.cmd_tx.clone());

        peer
    }
}
