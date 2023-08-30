use std::io::ErrorKind;
use std::net::{SocketAddr, UdpSocket};
use std::ops::{Deref, DerefMut};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use futures::channel::mpsc;
use str0m::{
    Rtc,
    net::{DatagramRecv, Receive, Transmit},
    Input, Output, Event
};

use crate::util::udp_conn::{EngineCommand, EngineEvent};

pub struct PeerConnection {
    id: u64,
    pub rtc: Arc<Mutex<Rtc>>,
}

impl PeerConnection {
    pub fn new(id: u64, tx: mpsc::UnboundedSender<EngineCommand>, mut rx: mpsc::UnboundedReceiver<EngineEvent>) -> Self {
        let rtc = Arc::new(Mutex::new(Rtc::builder().build()));

        tokio::spawn({
            let rtc = Arc::clone(&rtc);
            async move {
                loop {
                    tokio::time::sleep(Duration::from_millis(5)).await;

                    let mut rtc = rtc.lock().unwrap();

                    assert!(rtc.is_alive());

                    rtc.handle_input(Input::Timeout(Instant::now())).unwrap();

                    loop {
                        match rtc.poll_output().unwrap() {
                            Output::Transmit(transmit) => {
                                tx.unbounded_send(EngineCommand::Transmit(transmit)).unwrap();
                            }
                            Output::Timeout(t) => {
                                break;
                            },
                            Output::Event(e) => match e {
                                Event::IceConnectionStateChange(v) => {
                                    error!("IceConnectionStateChange");
                                }
                                Event::MediaAdded(e) => {
                                    error!("MediaAdded");
                                    // self.handle_media_added(e.mid, e.kind)
                                },
                                Event::MediaData(data) => {
                                    error!("MediaData");
                                    // Propagated::MediaData(self.id, data)
                                },
                                Event::KeyframeRequest(req) => {
                                    error!("KeyframeRequest");
                                    // self.handle_incoming_keyframe_req(req)
                                },
                                Event::ChannelOpen(cid, _) => {
                                    error!("ChannelOpen");
                                    // self.cid = Some(cid);
                                    // Propagated::Noop
                                }
                                Event::ChannelData(data) => {
                                    error!("ChannelData");
                                    // self.handle_channel_data(data)
                                },
                                Event::Connected => {
                                    error!("Connected");
                                    // self.handle_channel_data(data)
                                },
                                Event::MediaChanged(_) => {
                                    error!("MediaChanged");
                                    // self.handle_channel_data(data)
                                },
                                Event::ChannelClose(_) => {
                                    error!("ChannelClose");
                                    // self.handle_channel_data(data)
                                },
                                Event::StreamPaused(_) => {
                                    error!("StreamPaused");
                                    // self.handle_channel_data(data)
                                },
                                Event::RtpPacket(_) => {
                                    error!("RtpPacket");
                                    // self.handle_channel_data(data)
                                },
                                _ => {}
                            },
                        }
                    }

                    match rx.try_next() {
                        Ok(Some(event)) => match event {
                            EngineEvent::IncomingNetworkData { at, data, source, destination } => {
                                let input = Input::Receive(
                                    Instant::now(),
                                    Receive {
                                        source,
                                        destination,
                                        contents: DatagramRecv::try_from(data.as_slice()).unwrap(),
                                    },
                                );
                                rtc.handle_input(input).unwrap();
                            }
                            EngineEvent::IncomingUnauthorizedNetworkData { at, data, source, destination } => {
                                let input = Input::Receive(
                                    Instant::now(),
                                    Receive {
                                        source,
                                        destination,
                                        contents: DatagramRecv::try_from(data.as_slice()).unwrap(),
                                    },
                                );

                                if rtc.accepts(&input) {
                                    rtc.handle_input(input).unwrap();

                                    tx.unbounded_send(EngineCommand::RegisterPeer((id, source))).unwrap();
                                }
                            }
                        },
                        Ok(None) => {
                            break;
                        }
                        Err(err) => {
                            continue;
                        }
                    }
                }
            }
        });

        PeerConnection {
            id,
            rtc,
        }
    }
}