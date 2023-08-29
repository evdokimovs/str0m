use std::net::SocketAddr;
use str0m::{Candidate, Input, Rtc};
use std::str::FromStr;
use std::time::Instant;
use str0m::media::{Direction, MediaKind};

mod udp_conn;

// mod udp_conn;

fn main() {
    // let mut rtc = Rtc::new();
    let mut rtc = Rtc::builder().set_ice_lite(false).build();
    rtc.add_local_candidate(Candidate::host(SocketAddr::from_str("127.0.0.1:8080").unwrap()).unwrap());
    rtc.handle_input(Input::Timeout(Instant::now()));
    rtc.handle_input(Input::Timeout(Instant::now()));
    rtc.handle_input(Input::Timeout(Instant::now()));
    let mut sdp_api = rtc.sdp_api();
    let mid = sdp_api.add_media(MediaKind::Audio, Direction::Inactive, None, None);
    let (sdp, state) = sdp_api.apply().unwrap();
    println!("{:?}", sdp);
    println!("{:?}", rtc.poll_output());
    println!("{:?}", rtc.poll_output());
    println!("{:?}", rtc.poll_output());
}
