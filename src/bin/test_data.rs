use str0m::Rtc;
use std::time::Instant;

fn main() {
    let mut rtc = Rtc::builder().build(Instant::now());
    
    let config = str0m::channel::ChannelConfig {
        label: "chat".to_string(),
        ordered: true,
        reliability: Default::default(),
        protocol: "".to_string(),
        negotiated: None,
    };
    
    let ch = rtc.direct_api().create_data_channel(config);
    
    let mut change = rtc.sdp_api();
    if change.has_changes() {
        if let Some((offer, pending)) = change.apply() {
            println!("Offer generated!");
        }
    }
}
