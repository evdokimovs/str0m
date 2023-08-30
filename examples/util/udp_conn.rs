use std::collections::HashMap;
use std::io::ErrorKind;
use std::net::{IpAddr, SocketAddr, UdpSocket};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use futures::channel::mpsc;
use str0m::{
    Rtc,
    net::{DatagramRecv, Receive, Transmit},
    Input,
};

use crate::util::peer::PeerConnection;

pub enum EngineEvent {
    IncomingNetworkData {
        at: Instant,
        data: Vec<u8>,
        source: SocketAddr,
        destination: SocketAddr
    },

    IncomingUnauthorizedNetworkData {
        at: Instant,
        data: Arc<Vec<u8>>,
        source: SocketAddr,
        destination: SocketAddr
    },
}

pub enum EngineCommand {
    RegisterPeer((u64, SocketAddr)),
    Transmit(Transmit)
}

pub struct PeerConnectionEngine {
    id: Mutex<u64>,

    pub local_addr: SocketAddr,

    connecting_peers: Arc<Mutex<HashMap<u64, mpsc::UnboundedSender<EngineEvent>>>>,
    event_tx: HashMap<SocketAddr, mpsc::UnboundedSender<EngineEvent>>,

    cmd_tx: mpsc::UnboundedSender<EngineCommand>,
    loop_handle: std::thread::JoinHandle<()>,
}

impl PeerConnectionEngine {
    pub fn new(bind_addr: IpAddr) -> Self {
        let (cmd_tx, mut cmd_rx) = mpsc::unbounded();
        let socket = UdpSocket::bind(format!("{bind_addr}:19305")).unwrap();
        let local_addr = socket.local_addr().unwrap();
        let mut peers_tx = HashMap::<SocketAddr, mpsc::UnboundedSender<EngineEvent>>::new();
        let connecting_peers = Arc::new(Mutex::new(HashMap::<u64, mpsc::UnboundedSender<EngineEvent>>::new()));

        let cp = Arc::clone(&connecting_peers);
        let loop_handle = std::thread::spawn(move || {
            loop {
                let mut read_buffer = vec![0; 2000];

                match cmd_rx.try_next() {
                    Ok(Some(event)) => match event {
                        EngineCommand::RegisterPeer((id, dst)) => {
                            let peer = cp.lock().unwrap().remove(&id).unwrap();
                            peers_tx.insert(dst, peer);
                        },
                        EngineCommand::Transmit(t) => {
                            socket.send_to(&t.contents, t.destination).unwrap();
                        }
                    },
                    Ok(None) => {
                        break;
                    }
                    Err(err) => { }
                }

                socket
                    .set_read_timeout(Some(Duration::from_micros(10)))
                    .expect("setting socket read timeout");

                let (data, src, at) = match socket.recv_from(&mut read_buffer) {
                    Ok((n, source)) => {
                        (read_buffer[0..n].to_vec(), source, Instant::now())
                    }

                    Err(e) => match e.kind() {
                        // Expected error for set_read_timeout(). One for windows, one for the rest.
                        ErrorKind::WouldBlock | ErrorKind::TimedOut => {
                            continue;
                        }
                        _ => panic!("UdpSocket read failed: {e:?}"),
                    },
                };

                if let Some(mut peer_tx) = peers_tx.get(&src) {
                    // error!("IncomingNetworkData from {src}", );
                    peer_tx.unbounded_send(EngineEvent::IncomingNetworkData {
                        at,
                        data,
                        source: src,
                        destination: local_addr,
                    }).unwrap();
                } else {
                    let mut peers = cp.lock().unwrap();
                    let data = Arc::new(data);

                    for (id, p) in peers.iter_mut() {
                        p.unbounded_send(EngineEvent::IncomingUnauthorizedNetworkData {
                            at,
                            data: Arc::clone(&data),
                            source: src,
                            destination: local_addr,
                        }).unwrap();
                    }
                }
            }
        });

        Self {
            id: Mutex::new(0),
            local_addr,
            connecting_peers,
            event_tx: HashMap::new(),
            cmd_tx,
            loop_handle: loop_handle,
        }
    }

    pub fn create_peer(&self) -> PeerConnection {
        let mut id = self.id.lock().unwrap();
        *id += 1;

        let (cmd_tx, cmd_rx) = mpsc::unbounded();
        let peer = PeerConnection::new(*id, self.cmd_tx.clone(), cmd_rx);

        self.connecting_peers.lock().unwrap().insert(*id, cmd_tx);

        peer
    }
}
