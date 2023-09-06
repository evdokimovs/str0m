mod local_track;
mod remote_track;
mod transceiver;

use std::collections::{HashMap, HashSet};
use std::future::Future;
use std::net::SocketAddr;
use std::pin::Pin;
use std::time::Duration;
use std::time::Instant;
use str0m::net::Receive;
use str0m::{Event, Output, RtcError};
use tokio::sync::mpsc;
use tokio::time::timeout;

use str0m::change::{SdpAnswer, SdpOffer, SdpPendingOffer};
use str0m::channel::{ChannelData, ChannelId};
use str0m::media::{Direction, MediaAdded, MediaData, Mid};
use str0m::Input;
use str0m::{Candidate, Rtc};
use str0m::media::MediaKind;
use tokio::sync::oneshot;

use crate::util::proto::{EngineCommand, EngineEvent, PeerId};

pub use self::local_track::LocalTrack;
pub use self::remote_track::RemoteTrack;
pub use self::transceiver::Transceiver;

pub type OnChannelDataHdlrFn = Box<
    dyn (FnMut(ChannelData) -> Pin<Box<dyn Future<Output = ()> + Send + 'static>>) + Send + Sync,
>;

pub struct PeerConnectionFactory {
    tx: mpsc::UnboundedSender<EngineCommand>,
    last_peer_id: PeerId,
}

impl PeerConnectionFactory {
    pub fn new(tx: mpsc::UnboundedSender<EngineCommand>) -> Self {
        Self {
            tx,
            last_peer_id: PeerId(0),
        }
    }

    pub fn create_peer(&mut self) -> PeerConnectionHandle {
        self.last_peer_id.0 += 1;
        PeerConnection::new(self.last_peer_id, self.tx.clone())
    }
}

#[derive(Debug, Clone)]
pub struct PeerConnectionHandle(mpsc::UnboundedSender<EngineEvent>, PeerId);

impl PeerConnectionHandle {
    pub fn peer_id(&self) -> PeerId {
        self.1
    }

    pub async fn add_local_candidate(&self, candidate: Candidate) {
        let (tx, rx) = oneshot::channel();
        self.0.send(EngineEvent::LocalCandidateAdded(candidate, tx));
    }

    pub async fn add_remote_candidate(&self, candidate: Candidate) {
        let (tx, rx) = oneshot::channel();
        self.0
            .send(EngineEvent::RemoteCandidateAdded(candidate, tx));
        rx.await;
    }

    pub fn on_remote_track(&self) -> mpsc::UnboundedReceiver<(RemoteTrack, Transceiver)> {
        let (tx, rx) = mpsc::unbounded_channel();
        self.0.send(EngineEvent::SubscriberToOnRemoteTrack(tx));
        rx
    }

    pub fn on_local_track(&self) -> mpsc::UnboundedReceiver<(LocalTrack, Transceiver)> {
        let (tx, rx) = mpsc::unbounded_channel();
        self.0.send(EngineEvent::SubscriberToOnLocalTrack(tx));
        rx
    }

    pub fn on_channel_data(&self, mut f: OnChannelDataHdlrFn) {
        let (tx, mut rx) = mpsc::unbounded_channel();
        self.0.send(EngineEvent::SubscribeToDataChannel(tx));
        tokio::spawn(async move {
            while let Some(data) = rx.recv().await {
                f(data).await;
            }
        });
    }

    pub async fn send_channel_data(&self, data: Vec<u8>) {
        let (tx, rx) = oneshot::channel();
        self.0.send(EngineEvent::WriteChannelData(data, tx));
        rx.await;
    }

    pub async fn accept_offer(&self, offer: SdpOffer) -> Result<SdpAnswer, RtcError> {
        let (tx, rx) = oneshot::channel();
        self.0.send(EngineEvent::OfferReceived(offer, tx));
        rx.await.unwrap()
    }

    pub async fn accept_answer(&self, answer: SdpAnswer) {
        let (tx, rx) = oneshot::channel();
        self.0.send(EngineEvent::AnswerReceived(answer, tx));
        rx.await.unwrap()
    }

    pub async fn add_transceivers(&self, transceivers: Vec<(MediaKind, Direction, String)>) -> Option<SdpOffer> {
        let (tx, rx) = oneshot::channel();
        self.0.send(EngineEvent::AddTransceivers(transceivers, tx));
        rx.await.unwrap()
    }
}

#[derive(Debug)]
pub struct PeerConnection {
    event_receiver: mpsc::UnboundedReceiver<EngineEvent>,
    event_sender: mpsc::UnboundedSender<EngineEvent>,
    command_sender: mpsc::UnboundedSender<EngineCommand>,
    pending_offer: Option<SdpPendingOffer>,
    source_addr: Option<SocketAddr>,
    rtc: Rtc,
    cid: Option<ChannelId>,
    peer_id: PeerId,
    channel_data_subscriber: Option<mpsc::UnboundedSender<ChannelData>>,
    on_remote_track_sub: Option<mpsc::UnboundedSender<(RemoteTrack, Transceiver)>>,
    on_local_track_sub: Option<mpsc::UnboundedSender<(LocalTrack, Transceiver)>>,
    remote_track_sub: HashMap<Mid, mpsc::UnboundedSender<MediaData>>,
    known_mids: HashSet<Mid>,
}

impl PeerConnection {
    pub fn new(
        peer_id: PeerId,
        command_sender: mpsc::UnboundedSender<EngineCommand>,
    ) -> PeerConnectionHandle {
        let (event_sender, event_receiver) = mpsc::unbounded_channel();
        command_sender.send(EngineCommand::PeerCreated(peer_id, event_sender.clone()));

        let this = Self {
            rtc: Rtc::new(),
            peer_id,
            event_sender: event_sender.clone(),
            event_receiver,
            command_sender,
            cid: None,
            source_addr: None,
            pending_offer: None,
            channel_data_subscriber: None,
            on_remote_track_sub: None,
            on_local_track_sub: None,
            remote_track_sub: HashMap::new(),
            known_mids: HashSet::new(),
        };

        this.spawn();

        PeerConnectionHandle(event_sender, peer_id)
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
            EngineEvent::PacketReceived {
                at,
                source,
                destination,
                payload: contents,
            } => {
                if self.source_addr.is_none() {
                    self.source_addr = Some(source);
                }
                let Ok(contents) = contents.as_slice().try_into() else {
                    return;
                };

                let input = Input::Receive(
                    at,
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
                let answer = self.rtc.sdp_api().accept_offer(offer);
                resolver.send(answer);
            }
            EngineEvent::AnswerReceived(answer, resolver) => {
                println!("MIDS [before]: {:?}", self.rtc.mids());
                if let Some(pending) = self.pending_offer.take() {
                    self.rtc.sdp_api().accept_answer(pending, answer).unwrap();
                }
                println!("MIDS [after]: {:?}", self.rtc.mids());
                for mid in self.rtc.mids() {
                    if !self.known_mids.contains(&mid) {
                        let media = self.rtc.media(mid).unwrap();
                        self.handle_rtc_event(str0m::Event::MediaAdded(MediaAdded {
                            mid: media.mid(),
                            kind: media.kind(),
                            direction: media.direction(),
                            simulcast: None,
                        }));
                    }
                }

                resolver.send(());
            }
            EngineEvent::WriteMediaData(data) => {
                if let Some(writer) = self.rtc.writer(data.mid) {
                    println!("Writer found");
                    let Some(pt) = writer.match_params(data.params) else {
                        panic!("Write not matches params");
                        return;
                    };
                    writer.write(pt, data.network_time, data.time, &data.data);
                } else {
                    println!("Can't find MID");
                }
            }
            EngineEvent::WriteChannelData(data, tx) => {
                let mut channel = self
                    .cid
                    .and_then(|id| self.rtc.channel(id))
                    .expect("channel to be open");
                channel.write(false, &data).expect("to write answer");
                tx.send(());
            }
            EngineEvent::SubscribeToDataChannel(data_channel_subscriber) => {
                assert!(self
                    .channel_data_subscriber
                    .replace(data_channel_subscriber)
                    .is_none());
            }
            EngineEvent::SubscriberToOnRemoteTrack(tx) => {
                assert!(self.on_remote_track_sub.replace(tx).is_none());
            }
            EngineEvent::SubscriberToOnLocalTrack(tx) => {
                assert!(self.on_local_track_sub.replace(tx).is_none());
            }
            EngineEvent::SubsriberRemoteTrack(mid, sub) => {
                assert!(self.remote_track_sub.insert(mid, sub).is_none());
            }
            EngineEvent::AddTransceivers(transceivers, tx) => {
                println!("MIDS [before]: {:?}", self.rtc.mids());
                {
                    let mut sdp_api = self.rtc.sdp_api();
                    for (kind, direction, stream_id) in transceivers {
                        sdp_api.add_media(kind, direction, Some(stream_id), None);
                    }
                    if let Some((offer, pending)) = sdp_api.apply() {
                        self.pending_offer = Some(pending);
                        tx.send(Some(offer));
                    } else {
                        tx.send(None);
                    }
                }
                println!("MIDS [after]: {:?}", self.rtc.mids());
            }
        }
    }

    fn poll_output(&mut self) {
        if let Ok(output) = self.rtc.poll_output() {
            match output {
                Output::Transmit(transmit) => {
                    self.command_sender.send(EngineCommand::Transmit(transmit));
                }
                Output::Timeout(timeout) => {}
                Output::Event(event) => {
                    self.handle_rtc_event(event);
                }
            }
        }
    }

    fn handle_rtc_event(&mut self, rtc_event: str0m::Event) {
        use str0m::Event;
        println!("MIDS: {:?}", self.rtc.mids());
        match rtc_event {
            Event::MediaData(data) => {
                // println!("MediaData received");
                if let Some(sub) = self.remote_track_sub.get(&data.mid) {
                    sub.send(data);
                }
            }
            Event::ChannelData(data) => {
                if let Some(sub) = &self.channel_data_subscriber {
                    sub.send(data);
                }
            }
            Event::KeyframeRequest(req) => {
                req.kind
            }
            Event::MediaAdded(media) => {
                println!("MediaAdded event sent: {:?}", media);
                let transceiver =
                    Transceiver::new(media.mid, media.direction, self.event_sender.clone());
                if media.direction.is_sending() && media.direction.is_receiving() {
                    panic!();
                }
                self.known_mids.insert(media.mid);
                if media.direction.is_receiving() {
                    println!("RemoteTrack");
                    if let Some(sub) = &self.on_remote_track_sub {
                        sub.send((
                            RemoteTrack::new(self.event_sender.clone(), media.mid, media.kind, media.direction),
                            transceiver.clone(),
                        ));
                    }
                }
                if media.direction.is_sending() {
                    if let Some(sub) = &self.on_local_track_sub {
                        println!("LocalTrack");
                        sub.send((
                            LocalTrack::new(self.event_sender.clone(), media.mid, media.kind, media.direction),
                            transceiver.clone(),
                        ));
                    }
                }
            }
            Event::ChannelOpen(cid, _) => {
                println!("ChannelOpen event sent");
                assert!(self.cid.replace(cid).is_none());
            }
            Event::Connected => {
                println!("Connected Peer");
                if let Some(source_addr) = self.source_addr {
                    self.command_sender
                        .send(EngineCommand::PeerConnected(self.peer_id, source_addr));
                }
            }
            _ => (),
        }
    }
}
