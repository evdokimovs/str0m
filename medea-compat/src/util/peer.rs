use std::future::Future;
use std::net::SocketAddr;
use std::pin::Pin;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tokio::sync::{mpsc, oneshot};
use tokio::time::timeout;

use str0m::change::{SdpAnswer, SdpOffer, SdpPendingOffer};
use str0m::channel::{ChannelData, ChannelId};
use str0m::media::MediaData;
use str0m::net::Receive;
use str0m::{Candidate, Input, Rtc, RtcError};
use str0m::{Event, Output};

use crate::util::proto::{EngineCommand, EngineEvent, PeerId};

pub type OnMediaDataHdlrFn =
    Box<dyn (FnMut(MediaData) -> Pin<Box<dyn Future<Output = ()> + Send + 'static>>) + Send + Sync>;

pub type OnChannelDataHdlrFn = Box<
    dyn (FnMut(ChannelData) -> Pin<Box<dyn Future<Output = ()> + Send + 'static>>) + Send + Sync,
>;

#[derive(Clone)]
pub struct PeerHandle {
    tx: mpsc::UnboundedSender<EngineEvent>,
    media_data_cb: Arc<tokio::sync::Mutex<Option<OnMediaDataHdlrFn>>>,
    channel_data_cb: Arc<tokio::sync::Mutex<Option<OnChannelDataHdlrFn>>>,
}

impl PeerHandle {
    pub async fn on_channel_data(&mut self, cb: OnChannelDataHdlrFn) {
        self.channel_data_cb.lock().await.replace(cb);
    }

    pub async fn on_media_data(&mut self, cb: OnMediaDataHdlrFn) {
        self.media_data_cb.lock().await.replace(cb);
    }

    pub async fn add_local_candidate(&self, c: Candidate) {
        let (tx, rx) = oneshot::channel();

        self.tx
            .send(EngineEvent::LocalCandidateAdded(c, tx))
            .unwrap();

        rx.await.unwrap();
    }

    pub async fn accept_offer(&self, offer: SdpOffer) -> Result<SdpAnswer, RtcError> {
        let (tx, rx) = oneshot::channel();

        self.tx.send(EngineEvent::OfferReceived(offer, tx)).unwrap();

        rx.await.unwrap()
    }

    pub async fn send_channel_data(&self, data: Vec<u8>) {
        let (tx, rx) = oneshot::channel();

        self.tx
            .send(EngineEvent::WriteChannelData(data, tx))
            .unwrap();

        rx.await.unwrap()
    }
}

#[derive(Debug)]
pub struct PeerConnection {
    event_receiver: mpsc::UnboundedReceiver<EngineEvent>,
    command_sender: mpsc::UnboundedSender<EngineCommand>,
    pending_offer: Option<SdpPendingOffer>,
    source_addr: Option<SocketAddr>,
    rtc: Rtc,
    cid: Option<ChannelId>,
    peer_id: PeerId,
    media_tx: mpsc::UnboundedSender<MediaData>,
    channel_tx: mpsc::UnboundedSender<ChannelData>,
}

impl PeerConnection {
    pub fn new(
        peer_id: PeerId,
        command_sender: mpsc::UnboundedSender<EngineCommand>,
    ) -> PeerHandle {
        let (event_tx, event_rx) = mpsc::unbounded_channel();
        let (media_tx, mut media_rx) = mpsc::unbounded_channel();
        let (channel_tx, mut channel_rx) = mpsc::unbounded_channel();
        let media_data_cb: Arc<tokio::sync::Mutex<Option<OnMediaDataHdlrFn>>> =
            Arc::new(tokio::sync::Mutex::new(None));
        let channel_data_cb: Arc<tokio::sync::Mutex<Option<OnChannelDataHdlrFn>>> =
            Arc::new(tokio::sync::Mutex::new(None));

        command_sender
            .send(EngineCommand::PeerCreated(peer_id, event_tx.clone()))
            .unwrap();

        let this = Self {
            rtc: Rtc::new(),
            peer_id,
            event_receiver: event_rx,
            command_sender,
            cid: None,
            source_addr: None,
            pending_offer: None,
            media_tx,
            channel_tx,
        };

        this.spawn();

        tokio::spawn({
            let media_data_cb = Arc::clone(&media_data_cb);
            async move {
                while let Some(data) = media_rx.recv().await {
                    if let Some(cb) = media_data_cb.lock().await.as_mut() {
                        cb(data).await;
                    }
                }
            }
        });

        tokio::spawn({
            let channel_data_cb = Arc::clone(&channel_data_cb);
            async move {
                while let Some(data) = channel_rx.recv().await {
                    if let Some(cb) = channel_data_cb.lock().await.as_mut() {
                        cb(data).await;
                    }
                }
            }
        });

        PeerHandle {
            tx: event_tx,
            media_data_cb,
            channel_data_cb,
        }
    }

    fn spawn(mut self) {
        tokio::spawn(async move {
            loop {
                let event = timeout(Duration::from_millis(10), self.event_receiver.recv()).await;

                assert!(self.rtc.is_alive());

                self.rtc
                    .handle_input(Input::Timeout(Instant::now()))
                    .unwrap();

                self.poll_output();

                if let Ok(event) = event {
                    if let Some(event) = event {
                        self.handle_event(event);
                    } else {
                        break;
                    }
                }
            }
        });
    }

    fn handle_event(&mut self, event: EngineEvent) {
        match event {
            EngineEvent::RemoteCandidateAdded(candidate, resolver) => {
                self.rtc.add_remote_candidate(candidate);
                resolver.send(()).unwrap();
            }

            EngineEvent::LocalCandidateAdded(candidate, resolver) => {
                self.rtc.add_local_candidate(candidate);
                resolver.send(()).unwrap();
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
                    self.rtc.handle_input(input).unwrap();
                }
            }
            EngineEvent::OfferReceived(offer, resolver) => {
                let answer = self.rtc.sdp_api().accept_offer(offer);
                resolver.send(answer).unwrap();
            }
            EngineEvent::AnswerReceived(answer, resolver) => {
                if let Some(pending) = self.pending_offer.take() {
                    self.rtc.sdp_api().accept_answer(pending, answer).unwrap();
                }
                resolver.send(()).unwrap();
            }
            EngineEvent::WriteMediaData(data) => {
                if let Some(writer) = self.rtc.writer(data.mid) {
                    let Some(pt) = writer.match_params(data.params) else {
                        return;
                    };
                    writer
                        .write(pt, data.network_time, data.time, &data.data)
                        .unwrap();
                }
            }
            EngineEvent::WriteChannelData(data, resolver) => {
                let mut channel = self
                    .cid
                    .and_then(|id| self.rtc.channel(id))
                    .expect("channel to be open");
                channel.write(false, &data).expect("to write answer");

                resolver.send(()).unwrap();
            } // EngineEvent::SubscrbeToMediaChannel(media_data_subscriber) => {
              //     assert!(self.media_tx.replace(media_data_subscriber).is_none());
              // }
              // EngineEvent::SubscribeToDataChannel(data_channel_subscriber) => {
              //     assert!(self.channel_tx.replace(data_channel_subscriber).is_none());
              // }
        }
    }

    fn poll_output(&mut self) {
        loop {
            match self.rtc.poll_output().unwrap() {
                Output::Transmit(transmit) => {
                    self.command_sender
                        .send(EngineCommand::Transmit(transmit))
                        .unwrap();
                }
                Output::Timeout(timeout) => {
                    break;
                }
                Output::Event(event) => {
                    self.handle_rtc_event(event);
                }
            }
        }
    }

    fn handle_rtc_event(&mut self, rtc_event: str0m::Event) {
        match rtc_event {
            Event::MediaData(data) => {
                // log::error!("MediaData");
                self.media_tx.send(data).unwrap();
            }
            Event::ChannelData(data) => {
                log::error!("ChannelData");
                self.channel_tx.send(data).unwrap();
            }
            Event::ChannelOpen(cid, _) => {
                log::error!("ChannelOpen");
                assert!(self.cid.replace(cid).is_none());
            }
            Event::Connected => {
                log::error!("Connected");
                if let Some(source_addr) = self.source_addr {
                    self.command_sender
                        .send(EngineCommand::PeerConnected(self.peer_id, source_addr))
                        .unwrap();
                }
            }
            Event::IceConnectionStateChange(_) => {
                log::error!("IceConnectionStateChange");
            }
            Event::MediaAdded(_) => {
                log::error!("MediaAdded");
            }
            Event::MediaChanged(_) => {
                log::error!("MediaChanged");
            }
            Event::ChannelClose(_) => {
                log::error!("ChannelClose");
            }
            Event::PeerStats(_) => {
                log::error!("PeerStats");
            }
            Event::MediaIngressStats(_) => {
                log::error!("MediaIngressStats");
            }
            Event::MediaEgressStats(_) => {
                log::error!("MediaEgressStats");
            }
            Event::EgressBitrateEstimate(_) => {
                log::error!("EgressBitrateEstimate");
            }
            Event::KeyframeRequest(_) => {
                log::error!("KeyframeRequest");
            }
            Event::StreamPaused(_) => {
                log::error!("StreamPaused");
            }
            Event::RtpPacket(_) => {
                log::error!("RtpPacket");
            }
            _ => {
                log::error!("Unexpected event");
            }
        }
    }
}
