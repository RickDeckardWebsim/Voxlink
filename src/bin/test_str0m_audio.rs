use str0m::{Rtc, media::{MediaKind, Direction, MediaTime}, bwe::BweKind};
use std::time::Instant;

fn main() {
    let mut rtc = Rtc::builder().build(Instant::now());
    let mid = rtc.sdp_api().add_media(MediaKind::Audio, Direction::SendRecv, None, None);
    
    if let Some(mut media) = rtc.media(mid) {
        let pt = media.payload_params()[0].pt();
        if let Some(mut writer) = media.writer() {
            let rtp_time = MediaTime::new(0, 48000);
            let _ = writer.write(pt, Instant::now(), rtp_time, &[0u8; 10]);
        }
    }
}
