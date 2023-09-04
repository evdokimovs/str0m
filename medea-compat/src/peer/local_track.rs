use std::collections::HashMap;
use std::net::SocketAddr;
use std::time::Duration;
use std::time::Instant;
use str0m::net::Receive;
use str0m::{Event, Output};
use tokio::sync::mpsc;
use tokio::time::timeout;

use str0m::change::{SdpAnswer, SdpOffer, SdpPendingOffer};
use str0m::channel::{ChannelData, ChannelId};
use str0m::media::{Direction, MediaData, MediaKind, Mid};
use str0m::Input;
use str0m::{Candidate, Rtc};
use tokio::sync::oneshot;

use crate::util::proto::{EngineCommand, EngineEvent, PeerId};
#[derive(Debug)]
pub struct LocalTrack {
    tx: mpsc::UnboundedSender<EngineEvent>,
    mid: Mid,
    direction: Direction,
    kind: MediaKind,
}

impl LocalTrack {
    pub fn new(tx: mpsc::UnboundedSender<EngineEvent>, mid: Mid, kind: MediaKind, direction: Direction) -> Self {
        Self { tx, mid, kind, direction }
    }

    pub fn write_rtp(&self, media_data: MediaData) {
        self.tx.send(EngineEvent::WriteMediaData(media_data));
    }

    pub fn kind(&self) -> MediaKind {
        self.kind
    }

    pub fn direction(&self) -> Direction {
        self.direction
    }
}
