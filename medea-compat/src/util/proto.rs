use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Instant;
use tokio::sync::{mpsc, oneshot};

use str0m::change::{SdpAnswer, SdpOffer};
use str0m::media::MediaData;
use str0m::net::Transmit;
use str0m::{Candidate, RtcError};

#[derive(Debug, Clone, Copy, Hash, Eq, PartialEq)]
pub struct PeerId(pub u32);

pub enum EngineEvent {
    RemoteCandidateAdded(Candidate, oneshot::Sender<()>),
    LocalCandidateAdded(Candidate, oneshot::Sender<()>),
    PacketReceived {
        at: Instant,
        source: SocketAddr,
        destination: SocketAddr,
        payload: Vec<u8>,
    },
    OfferReceived(SdpOffer, oneshot::Sender<Result<SdpAnswer, RtcError>>),
    AnswerReceived(SdpAnswer, oneshot::Sender<()>),
    WriteMediaData(MediaData),
    WriteChannelData(Vec<u8>, oneshot::Sender<()>),
}

pub enum EngineCommand {
    Transmit(Transmit),
    PeerCreated(PeerId, mpsc::UnboundedSender<EngineEvent>),
    PeerConnected(PeerId, SocketAddr),
}

// pub struct EngineEventReceiver {
//     rx: mpsc::UnboundedReceiver<EngineEventMsg>,
// }
//
// impl EngineEventReceiver {
//     pub async fn receive(&self) -> Option<EngineEventMsg> {
//         self.rx.recv().await
//     }
// }
//
// pub struct EngineEventChannel {
//     tx: mpsc::UnboundedSender<EngineEventMsg>,
// }
//
// #[async_trait]
// trait EngineEventSender<T> {
//     async fn send(&self, event: EngineEvent) -> anyhow::Result<T>;
// }
//
// #[async_trait]
// impl EngineEventSender<()> for EngineEventChannel {
//     async fn send(&self, event: EngineEvent) -> anyhow::Result<()> {
//         let (tx, rx) = oneshot::channel();
//         self.tx.send(EngineEventMsg {
//             event,
//             ret: EngineEventResolver::Void(tx),
//         });
//         Ok(rx.await?)
//     }
// }
//
// struct Resolver<T> {
//     ty: PhantomData<T>,
//     tx: oneshot::Sender<T>,
// }
//
// impl<T> Resolver<T> {
//     fn resolve(self, res: T) {
//         self.tx.send(res);
//     }
// }
//
// struct NewPeer {
//     peer_id: u32,
//     resolver: Resolver<()>,
// }
//
// impl Message<()> for NewPeer {
//     fn resolve(self, res: ()) {
//         self.resolver.resolve(res);
//     }
// }
//
// trait Message<T> {
//     fn resolve(self, res: T);
// }
//
// enum EngineEventMsg {
//     NewPeer(NewPeer),
// }
//
// // pub struct EngineEventMsg {
// //     event: EngineEvent,
// //     ret: EngineEventResolver,
// // }
//
// pub enum EngineEventResolver {
//     Void(oneshot::Sender<()>),
// }
//
// pub enum EngineEventOutput {
//     Void(oneshot::Receiver<()>),
// }
//
// pub enum EngineEvent {
//
// }
//
// pub enum EngineCommand {
// }
