use anyhow::{Context as AnyhowCtx, Result};
use std::net::SocketAddr;
use std::time::Instant;
use tokio::net::UdpSocket;
use str0m::{Rtc, Event, Output, Input, Candidate};
use str0m::media::{MediaKind, Direction, MediaTime};
use crate::audio::{capture::start_capture, playback::start_playback};

use crate::state::{NetEvent, UiCommand};

pub enum SignalingMsg {
    PeerJoined(String),
    PeerLeft(String),
    Offer { from: String, sdp: String },
    Answer { from: String, sdp: String },
}

pub async fn run(
    username: String,
    net_tx: std::sync::mpsc::Sender<NetEvent>,
    mut cmd_rx: tokio::sync::mpsc::UnboundedReceiver<UiCommand>,
    ctx: egui::Context,
) -> Result<()> {
    log::info!("[webrtc] Task started for '{}'", username);

    let socket = UdpSocket::bind("0.0.0.0:0").await.context("Failed to bind UDP socket")?;
    let local_addr = socket.local_addr()?;
    log::info!("[webrtc] Bound UDP socket to {}", local_addr);

    let stun_candidate = match discover_stun_candidate(&socket).await {
        Ok(addr) => {
            log::info!("[webrtc] Discovered STUN candidate: {}", addr);
            Some(addr)
        }
        Err(e) => {
            log::warn!("[webrtc] STUN discovery failed: {}. Falling back to host only.", e);
            None
        }
    };

    // We only support 1 peer in this iteration to keep str0m state machine simple.
    // Full mesh would require HashMap<String, Rtc>.
    let mut rtc = Rtc::builder().build(Instant::now());
    
    // Add local candidates
    if let Ok(c) = Candidate::host(local_addr, "udp") {
        let _ = rtc.add_local_candidate(c);
    }
    if let Some(stun_addr) = stun_candidate {
        if let Ok(c) = Candidate::server_reflexive(stun_addr, stun_addr, "udp") {
            let _ = rtc.add_local_candidate(c);
        }
    }

    let (sig_tx, mut sig_rx) = tokio::sync::mpsc::unbounded_channel::<SignalingMsg>();
    let (sig_cmd_tx, sig_cmd_rx) = tokio::sync::mpsc::unbounded_channel::<crate::net::signaling::SigCmd>();

    // Spawn signaling task
    tokio::spawn(crate::net::signaling::run_signaling(
        username.clone(),
        net_tx.clone(),
        sig_cmd_rx,
        sig_tx,
        ctx.clone(),
    ));

    let mut buf = vec![0u8; 2000];
    let mut active_peer: Option<String> = None;
    let mut data_channel = None;
    let mut pending_offer = None;

    let audio_mid = Some(rtc.sdp_api().add_media(MediaKind::Audio, Direction::SendRecv, None, None, None));
    let mut rtp_counter = 0;
    let mut mic_active = false;
    
    let (mic_tx, mut mic_rx) = tokio::sync::mpsc::unbounded_channel::<Vec<u8>>();
    let mut speaker_tx: Option<tokio::sync::mpsc::UnboundedSender<Vec<u8>>> = None;

    loop {
        // 1. Drain str0m output
        while let Ok(output) = rtc.poll_output() {
            match output {
                Output::Transmit(t) => {
                    let _ = socket.send_to(&t.contents, t.destination).await;
                }
                Output::Event(e) => {
                    match e {
                        Event::Connected => {
                            log::info!("[webrtc] P2P Connected!");
                            // Start audio streams
                            if let Ok(()) = start_capture(mic_tx.clone()) {
                                log::info!("[webrtc] Audio capture running.");
                            }
                            if let Ok(tx) = start_playback() {
                                speaker_tx = Some(tx);
                            }
                        }
                        Event::ChannelOpen(id, name) => {
                            log::info!("[webrtc] Data channel opened: {}", name);
                            data_channel = Some(id);
                        }
                        Event::ChannelData(data) => {
                            if let Ok(msg) = String::from_utf8(data.data) {
                                let from = active_peer.clone().unwrap_or_else(|| "Peer".to_string());
                                let _ = net_tx.send(NetEvent::MessageReceived {
                                    from,
                                    content: msg,
                                });
                                ctx.request_repaint();
                            }
                        }
                        Event::MediaData(data) => {
                            if let Some(tx) = &speaker_tx {
                                let _ = tx.send(data.data.to_vec());
                            }
                        }
                        _ => {}
                    }
                }
                Output::Timeout(_t) => {
                    break;
                }
            }
        }

        // 2. Wait for next event
        let timeout = match rtc.poll_output() {
            Ok(Output::Timeout(t)) => tokio::time::sleep_until(tokio::time::Instant::from_std(t)),
            _ => tokio::time::sleep(std::time::Duration::from_secs(3600)),
        };

        tokio::select! {
            _ = timeout => {
                let _ = rtc.handle_input(Input::Timeout(Instant::now()));
            }

            result = socket.recv_from(&mut buf) => {
                if let Ok((n, addr)) = result {
                    if let Ok(contents) = buf[..n].try_into() {
                        let receive = str0m::net::Receive {
                            source: addr,
                            destination: local_addr,
                            contents,
                            proto: str0m::net::Protocol::Udp,
                        };
                        let _ = rtc.handle_input(Input::Receive(Instant::now(), receive));
                    }
                }
            }

            mic_packet = mic_rx.recv() => {
                if let Some(packet) = mic_packet {
                    if mic_active {
                        if let Some(mid) = audio_mid {
                            if let Some(writer) = rtc.writer(mid) {
                                let pt = writer.payload_params().next().map(|p| p.pt());
                                if let Some(pt) = pt {
                                    let freq: std::num::NonZeroU32 = std::num::NonZeroU32::new(48000).unwrap();
                                    let time = MediaTime::new(rtp_counter, freq.into());
                                    let _ = writer.write(pt, Instant::now(), time, packet);
                                }
                            }
                        }
                    }
                    // Always increment counter even if muted to maintain RTP timing
                    rtp_counter += 960; // 20ms at 48kHz
                }
            }

            sig = sig_rx.recv() => {
                if let Some(msg) = sig {
                    match msg {
                        SignalingMsg::PeerJoined(peer) => {
                            log::info!("[webrtc] Peer {} joined", peer);
                            active_peer = Some(peer.clone());
                            // If our username is lexicographically smaller, we initiate.
                            if username < peer {
                                log::info!("[webrtc] Initiating WebRTC offer to {}", peer);
                                let config = str0m::channel::ChannelConfig {
                                    label: "chat".to_string(),
                                    ordered: true,
                                    reliability: Default::default(),
                                    protocol: "".to_string(),
                                    negotiated: None,
                                };
                                let _ch = rtc.direct_api().create_data_channel(config);
                                
                                let change = rtc.sdp_api();
                                if let Some((offer, pending)) = change.apply() {
                                    pending_offer = Some(pending);
                                    let sdp = serde_json::to_string(&offer).unwrap();
                                    let _ = sig_cmd_tx.send(crate::net::signaling::SigCmd::SendOffer { to: peer, sdp });
                                }
                            }
                        }
                        SignalingMsg::PeerLeft(peer) => {
                            if Some(peer) == active_peer {
                                log::info!("[webrtc] Active peer left");
                                active_peer = None;
                                data_channel = None;
                                rtc.disconnect();
                            }
                        }
                        SignalingMsg::Offer { from, sdp } => {
                            if Some(from.clone()) != active_peer {
                                active_peer = Some(from.clone());
                            }
                            if let Ok(offer) = serde_json::from_str::<str0m::change::SdpOffer>(&sdp) {
                                log::info!("[webrtc] Received offer from {}", from);
                                if let Ok(answer) = rtc.sdp_api().accept_offer(offer) {
                                    let sdp = serde_json::to_string(&answer).unwrap();
                                    let _ = sig_cmd_tx.send(crate::net::signaling::SigCmd::SendAnswer { to: from, sdp });
                                }
                            }
                        }
                        SignalingMsg::Answer { from, sdp } => {
                            if let Ok(answer) = serde_json::from_str::<str0m::change::SdpAnswer>(&sdp) {
                                log::info!("[webrtc] Received answer from {}", from);
                                if let Some(pending) = pending_offer.take() {
                                    if let Err(e) = rtc.sdp_api().accept_answer(pending, answer) {
                                        log::warn!("[webrtc] Failed to accept answer: {:?}", e);
                                    } else {
                                        log::info!("[webrtc] SDP answer accepted, ICE should connect now.");
                                    }
                                } else {
                                    log::warn!("[webrtc] Received answer but no pending offer");
                                }
                            }
                        }
                    }
                }
            }

            cmd = cmd_rx.recv() => {
                if let Some(cmd) = cmd {
                    match cmd {
                        UiCommand::Disconnect => {
                            let _ = sig_cmd_tx.send(crate::net::signaling::SigCmd::Disconnect);
                            break;
                        }
                        UiCommand::SendMessage(content) => {
                            if let Some(cid) = data_channel {
                                if let Some(mut ch) = rtc.channel(cid) {
                                    let _ = ch.write(true, content.as_bytes());
                                }
                            } else {
                                log::warn!("[webrtc] No data channel open to send message");
                            }
                        }
                        UiCommand::ToggleVoice(active) => {
                            mic_active = active;
                            log::info!("[webrtc] Voice active: {}", active);
                        }
                        _ => {}
                    }
                } else {
                    break;
                }
            }
        }
    }
    
    Ok(())
}

async fn discover_stun_candidate(socket: &UdpSocket) -> Result<SocketAddr> {
    use tokio::net::lookup_host;
    let mut stun_addrs = lookup_host("stun.l.google.com:19302").await?;
    let stun_addr = stun_addrs.next().context("Failed to resolve STUN")?;

    let mut req = [0u8; 20];
    req[0] = 0x00; req[1] = 0x01; // Binding Request
    req[4..8].copy_from_slice(&[0x21, 0x12, 0xa4, 0x42]); // Magic Cookie
    // Use std rand for 12 bytes
    let rand_bytes: [u8; 12] = std::array::from_fn(|_| rand::random::<u8>());
    req[8..20].copy_from_slice(&rand_bytes);

    socket.send_to(&req, stun_addr).await?;

    let mut buf = [0u8; 1500];
    let (n, src) = tokio::time::timeout(std::time::Duration::from_secs(3), socket.recv_from(&mut buf)).await??;
    
    if src != stun_addr { return Err(anyhow::anyhow!("Unexpected STUN source")); }
    if n < 20 || buf[0..2] != [0x01, 0x01] { return Err(anyhow::anyhow!("Invalid STUN response")); }

    let mut i = 20;
    while i + 4 <= n {
        let attr_type = u16::from_be_bytes([buf[i], buf[i+1]]);
        let attr_len = u16::from_be_bytes([buf[i+2], buf[i+3]]) as usize;
        if i + 4 + attr_len > n { break; }
        
        if attr_type == 0x0020 { // XOR-MAPPED-ADDRESS
            let family = buf[i+5];
            let port = u16::from_be_bytes([buf[i+6], buf[i+7]]) ^ 0x2112;
            if family == 0x01 { // IPv4
                let ip_bytes = [
                    buf[i+8] ^ 0x21, buf[i+9] ^ 0x12, buf[i+10] ^ 0xa4, buf[i+11] ^ 0x42
                ];
                let ip = std::net::Ipv4Addr::from(ip_bytes);
                return Ok(SocketAddr::V4(std::net::SocketAddrV4::new(ip, port)));
            }
        }
        i += 4 + attr_len;
    }
    Err(anyhow::anyhow!("No mapped address found in STUN response"))
}
