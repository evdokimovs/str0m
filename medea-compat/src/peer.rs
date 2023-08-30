use std::net::SocketAddr;
use std::time::Duration;
use tokio::sync::mpsc;
use tokio::time::timeout;
use std::time::Instant;
use str0m::net::Receive;
use str0m::{Event, Output};

use str0m::Input;
use str0m::change::{SdpAnswer, SdpOffer, SdpPendingOffer};
use str0m::{Candidate, Rtc};
use str0m::channel::{ChannelData, ChannelId};
use str0m::media::MediaData;
use crate::proto::{EngineCommand, EngineEvent, PeerId};

#[derive(Debug)]
struct PeerConnection {
    event_receiver: mpsc::UnboundedReceiver<EngineEvent>,
    command_sender: mpsc::UnboundedSender<EngineCommand>,
    pending_offer: Option<SdpPendingOffer>,
    source_addr: Option<SocketAddr>,
    rtc: Rtc,
    cid: Option<ChannelId>,
    peer_id: PeerId,
    media_data_subscriber: Option<mpsc::UnboundedSender<MediaData>>,
    channel_data_subscriber: Option<mpsc::UnboundedSender<ChannelData>>,
}

impl PeerConnection {
    pub fn new(peer_id: PeerId, command_sender: mpsc::UnboundedSender<EngineCommand>) {
        let (event_sender, event_receiver) = mpsc::unbounded_channel();
        command_sender.send(EngineCommand::PeerCreated(peer_id, event_sender));

        let this = Self {
            rtc: Rtc::new(),
            peer_id,
            event_receiver,
            command_sender,
            cid: None,
            source_addr: None,
            pending_offer: None,
            media_data_subscriber: None,
            channel_data_subscriber: None,
        };

        this.spawn();
    }

    fn spawn(mut self) {
        tokio::spawn(async move {
            loop {
                let event = timeout(Duration::from_millis(10), self.event_receiver.recv()).await;
                self.poll_output();
                if let Ok(event) = event {
                    if let Some(event) = event {
                        self.handle_event(event);
                    } else {
                        break;
                    }
                } else {
                    self.handle_timeout();
                }
            }
        });
    }

    fn handle_timeout(&mut self) {
        self.rtc.handle_input(Input::Timeout(Instant::now()));
    }

    fn handle_event(&mut self, event: EngineEvent) {
        match event {
            EngineEvent::RemoteCandidateAdded(candidate, resolver) => {
                self.rtc.add_remote_candidate(candidate);
                resolver.send(());
            }

            EngineEvent::LocalCandidateAdded(candidate, resolver) => {
                self.rtc.add_local_candidate(candidate);
                resolver.send(());
            }

            EngineEvent::PacketReceived { source, destination, contents } => {
                if self.source_addr.is_none() {
                    self.source_addr = Some(source);
                }
                let Ok(contents) = contents.as_slice().try_into() else {
                    return;
                };

                let input = Input::Receive(
                    Instant::now(),
                    Receive {
                        source,
                        destination,
                        contents,
                    },
                );

                if self.rtc.accepts(&input) {
                    self.rtc.handle_input(input);
                }
            }
            EngineEvent::OfferReceived(offer, resolver) => {
                let answer = self.rtc.sdp_api()
                    .accept_offer(offer)
                    .unwrap();
                resolver.send(answer);
            }
            EngineEvent::AnswerReceived(answer, resolver) => {
                if let Some(pending) = self.pending_offer.take() {
                    self.rtc.sdp_api()
                        .accept_answer(pending, answer)
                        .unwrap();
                }
                resolver.send(());
            }
            EngineEvent::WriteMediaData(data) => {
                if let Some(writer) = self.rtc.writer(data.mid) {
                    let Some(pt) = writer.match_params(data.params) else {
                        return;
                    };
                    writer.write(pt, data.network_time, data.time, &data.data);
                }
            }
            EngineEvent::WriteData(data) => {
                let mut channel = self
                    .cid
                    .and_then(|id| self.rtc.channel(id))
                    .expect("channel to be open");
                channel
                    .write(false, &data)
                    .expect("to write answer");
            }
            EngineEvent::SubscrbeToMediaChannel(media_data_subscriber) => {
                assert!(self.media_data_subscriber.replace(media_data_subscriber).is_none());
            }
            EngineEvent::SubscribeToDataChannel(data_channel_subscriber) => {
                assert!(self.channel_data_subscriber.replace(data_channel_subscriber).is_none());
            }
        }
    }

    fn poll_output(&mut self) {
        if let Ok(output) = self.rtc.poll_output() {
            match output {
                Output::Transmit(transmit) => {
                    self.command_sender.send(EngineCommand::Transmit(transmit));
                }
                Output::Timeout(timeout) => {

                }
                Output::Event(event) => {
                    self.handle_rtc_event(event);
                }
            }
        }
    }

    fn handle_rtc_event(&mut self, rtc_event: str0m::Event) {
        use str0m::Event;
        match rtc_event {
            Event::MediaData(data) => {
                if let Some(sub) = &self.media_data_subscriber {
                    sub.send(data);
                }
            }
            Event::ChannelData(data) => {
                if let Some(sub) = &self.channel_data_subscriber {
                    sub.send(data);
                }
            }
            Event::ChannelOpen(cid, _) => {
                assert!(self.cid.replace(cid).is_none());
            }
            Event::Connected => {
                if let Some(source_addr) = self.source_addr {
                    self.command_sender.send(EngineCommand::PeerConnected(self.peer_id, source_addr));
                }
            }
            _ => ()
        }
    }
}
