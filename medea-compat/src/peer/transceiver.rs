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
use str0m::media::{Direction, MediaData, Mid};
use str0m::Input;
use str0m::{Candidate, Rtc};
use tokio::sync::oneshot;

use crate::util::proto::{EngineCommand, EngineEvent, PeerId};

#[derive(Debug, Clone)]
pub struct Transceiver {
    tx: mpsc::UnboundedSender<EngineEvent>,
    mid: Mid,
    direction: Direction,
}

impl Transceiver {
    pub fn new(mid: Mid, direction: Direction, tx: mpsc::UnboundedSender<EngineEvent>) -> Self {
        Self { tx, mid, direction }
    }
}
