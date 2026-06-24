use anyhow::{Context, Result};
use futures_util::{SinkExt, StreamExt};
use serde_json::{json, Value};
use tokio::sync::mpsc;
use tokio::time::{sleep, Duration, Instant};
use tokio_tungstenite::{connect_async, tungstenite::Message};

use crate::net::webrtc::SignalingMsg;
use crate::state::NetEvent;

// Supabase details
const WS_HOST: &str = "syftqwloslmnjyvppler.supabase.co";
const ANON_KEY: &str = "sb_publishable_VK3kO0lX4tTsrHlCsH6JFQ_ebB6_lMH";
const CHANNEL: &str  = "p2p-signaling";
const HEARTBEAT_S: u64 = 25;

pub enum SigCmd {
    SendOffer { to: String, sdp: String },
    SendAnswer { to: String, sdp: String },
    BroadcastPeerJoin,
    BroadcastMessage(String),
    BroadcastMedia { caption: String, url: String, kind: String, filename: String },
    /// Broadcast this user's microphone/voice state to all peers.
    BroadcastVoiceState { speaking: bool, muted: bool, in_voice: bool },
    Disconnect,
}

pub async fn run_signaling(
    username: String,
    net_tx: std::sync::mpsc::Sender<NetEvent>,
    mut sig_cmd_rx: mpsc::UnboundedReceiver<SigCmd>,
    webrtc_tx: mpsc::UnboundedSender<SignalingMsg>,
    ctx: egui::Context,
) {
    let mut backoff = 1;

    loop {
        log::info!("[signaling] Connecting...");
        match connect_and_run(&username, &net_tx, &mut sig_cmd_rx, &webrtc_tx, &ctx).await {
            Ok(true) => {
                log::info!("[signaling] Disconnected gracefully.");
                break;
            }
            Ok(false) | Err(_) => {
                let _ = net_tx.send(NetEvent::Disconnected);
                ctx.request_repaint();
                log::warn!("[signaling] Connection lost. Reconnecting in {}s...", backoff);
                sleep(Duration::from_secs(backoff)).await;
                backoff = (backoff * 2).min(30);
            }
        }
    }
}

async fn connect_and_run(
    username: &str,
    net_tx: &std::sync::mpsc::Sender<NetEvent>,
    sig_cmd_rx: &mut mpsc::UnboundedReceiver<SigCmd>,
    webrtc_tx: &mpsc::UnboundedSender<SignalingMsg>,
    ctx: &egui::Context,
) -> Result<bool> {
    let url = format!("wss://{}/realtime/v1/websocket?apikey={}&vsn=1.0.0", WS_HOST, ANON_KEY);
    let (mut ws_stream, _) = connect_async(&url).await.context("WS connect failed")?;

    let join_msg = json!({
        "topic": format!("realtime:{}", CHANNEL),
        "event": "phx_join",
        "payload": {
            "config": {
                "broadcast": { "self": true }
            }
        },
        "ref": "1"
    });
    send_text(&mut ws_stream, &join_msg.to_string()).await?;

    let mut last_heartbeat = Instant::now();
    let mut ref_count = 2;

    loop {
        if last_heartbeat.elapsed() > Duration::from_secs(HEARTBEAT_S) {
            let hb = json!({
                "topic": "phoenix",
                "event": "heartbeat",
                "payload": {},
                "ref": ref_count.to_string()
            });
            ref_count += 1;
            send_text(&mut ws_stream, &hb.to_string()).await?;
            last_heartbeat = Instant::now();
        }

        tokio::select! {
            msg_opt = ws_stream.next() => {
                let msg = match msg_opt {
                    Some(Ok(m)) => m,
                    _ => return Ok(false),
                };

                if let Some(text) = msg_to_text(msg) {
                    if let Ok(parsed) = serde_json::from_str::<Value>(&text) {
                        handle_incoming(parsed, username, net_tx, webrtc_tx, ctx);
                    }
                }
            }

            cmd_opt = sig_cmd_rx.recv() => {
                if let Some(cmd) = cmd_opt {
                    match cmd {
                        SigCmd::Disconnect => return Ok(true),
                        SigCmd::BroadcastPeerJoin => {
                            let topic = format!("realtime:{}", CHANNEL);
                            let broadcast = make_broadcast(&topic, "peer_join", json!({
                                "from": username,
                            }), &mut ref_count);
                            send_text(&mut ws_stream, &broadcast).await?;
                        }
                        SigCmd::SendOffer { to, sdp } => {
                            let topic = format!("realtime:{}", CHANNEL);
                            let broadcast = make_broadcast(&topic, "sdp_offer", json!({
                                "from": username,
                                "to": to,
                                "sdp": sdp
                            }), &mut ref_count);
                            send_text(&mut ws_stream, &broadcast).await?;
                        }
                        SigCmd::SendAnswer { to, sdp } => {
                            let topic = format!("realtime:{}", CHANNEL);
                            let broadcast = make_broadcast(&topic, "sdp_answer", json!({
                                "from": username,
                                "to": to,
                                "sdp": sdp
                            }), &mut ref_count);
                            send_text(&mut ws_stream, &broadcast).await?;
                        }
                        SigCmd::BroadcastMessage(content) => {
                            let topic = format!("realtime:{}", CHANNEL);
                            let broadcast = make_broadcast(&topic, "chat_message", json!({
                                "from": username,
                                "content": content,
                            }), &mut ref_count);
                            send_text(&mut ws_stream, &broadcast).await?;
                        }
                        SigCmd::BroadcastMedia { caption, url, kind, filename } => {
                            let topic = format!("realtime:{}", CHANNEL);
                            let broadcast = make_broadcast(&topic, "chat_media", json!({
                                "from": username,
                                "content": caption,
                                "url": url,
                                "kind": kind,
                                "filename": filename,
                            }), &mut ref_count);
                            send_text(&mut ws_stream, &broadcast).await?;
                        }
                        SigCmd::BroadcastVoiceState { speaking, muted, in_voice } => {
                            let topic = format!("realtime:{}", CHANNEL);
                            let broadcast = make_broadcast(&topic, "voice_state", json!({
                                "from":     username,
                                "speaking": speaking,
                                "muted":    muted,
                                "in_voice": in_voice,
                            }), &mut ref_count);
                            send_text(&mut ws_stream, &broadcast).await?;
                        }
                    }
                } else {
                    return Ok(true);
                }
            }
            
            _ = sleep(Duration::from_secs(1)) => {}
        }
    }
}

fn handle_incoming(
    parsed: Value,
    username: &str,
    net_tx: &std::sync::mpsc::Sender<NetEvent>,
    webrtc_tx: &mpsc::UnboundedSender<SignalingMsg>,
    ctx: &egui::Context,
) {
    let event = parsed["event"].as_str().unwrap_or("");
    let payload = &parsed["payload"];

    match event {
        "phx_reply" => {
            if payload["status"] == "ok" && parsed["ref"] == "1" {
                let _ = net_tx.send(NetEvent::Connected);
                ctx.request_repaint();
                
                // Broadcast presence join to other users in the channel
                // webrtc_tx doesn't need our own name, we need to send a SigCmd back to the loop.
                // We will rely on the caller to send SigCmd::BroadcastPeerJoin.
                // Wait, handle_incoming can't send SigCmd directly unless we pass sig_cmd_tx!
                // Let's just pass webrtc_tx the command to ask the main thread to broadcast.
                let _ = webrtc_tx.send(SignalingMsg::PeerJoined(username.to_string())); // Trigger self check
            }
        }
        "broadcast" => {
            let b_event = payload["event"].as_str().unwrap_or("");
            let b_payload = &payload["payload"];
            let from = b_payload["from"].as_str().unwrap_or("Unknown");
            let to = b_payload["to"].as_str().unwrap_or("");

            if from == username { return; }

            match b_event {
                "peer_join" => {
                    let _ = net_tx.send(NetEvent::PeerJoined(from.to_string()));
                    let _ = webrtc_tx.send(SignalingMsg::PeerJoined(from.to_string()));
                    ctx.request_repaint();
                }
                "peer_leave" => {
                    let _ = net_tx.send(NetEvent::PeerLeft(from.to_string()));
                    let _ = webrtc_tx.send(SignalingMsg::PeerLeft(from.to_string()));
                    ctx.request_repaint();
                }
                "sdp_offer" => {
                    if to == username {
                        if let Some(sdp) = b_payload["sdp"].as_str() {
                            let _ = webrtc_tx.send(SignalingMsg::Offer { from: from.to_string(), sdp: sdp.to_string() });
                        }
                    }
                }
                "sdp_answer" => {
                    if to == username {
                        if let Some(sdp) = b_payload["sdp"].as_str() {
                            let _ = webrtc_tx.send(SignalingMsg::Answer { from: from.to_string(), sdp: sdp.to_string() });
                        }
                    }
                }
                "chat_message" => {
                    if let Some(content) = b_payload["content"].as_str() {
                        let _ = net_tx.send(NetEvent::MessageReceived {
                            from: from.to_string(),
                            content: content.to_string(),
                            attachment: None,
                        });
                        ctx.request_repaint();
                    }
                }
                "voice_state" => {
                    let speaking = b_payload["speaking"].as_bool().unwrap_or(false);
                    let muted    = b_payload["muted"].as_bool().unwrap_or(false);
                    let in_voice = b_payload["in_voice"].as_bool().unwrap_or(false);
                    let _ = net_tx.send(NetEvent::VoiceStateUpdate {
                        from: from.to_string(),
                        speaking,
                        muted,
                        in_voice,
                    });
                    ctx.request_repaint();
                }
                "chat_media" => {
                    let content  = b_payload["content"].as_str().unwrap_or("").to_string();
                    let url      = b_payload["url"].as_str().unwrap_or("").to_string();
                    let kind_str = b_payload["kind"].as_str().unwrap_or("image");
                    let filename = b_payload["filename"].as_str().unwrap_or("attachment").to_string();

                    let kind = match kind_str {
                        "audio" => crate::state::AttachmentKind::Audio,
                        "video" => crate::state::AttachmentKind::Video,
                        _       => crate::state::AttachmentKind::Image,
                    };
                    let attachment = if url.is_empty() { None } else {
                        Some(crate::state::Attachment { url, kind, filename })
                    };
                    let _ = net_tx.send(NetEvent::MessageReceived {
                        from: from.to_string(),
                        content,
                        attachment,
                    });
                    ctx.request_repaint();
                }
                _ => {}
            }
        }
        _ => {}
    }
}

fn make_broadcast(topic: &str, event: &str, payload: Value, ref_count: &mut u64) -> String {
    let r = *ref_count;
    *ref_count += 1;
    json!({
        "topic": topic,
        "event": "broadcast",
        "payload": {
            "type": "broadcast",
            "event": event,
            "payload": payload
        },
        "ref": r.to_string(),
        "join_ref": "1"
    }).to_string()
}

async fn send_text<S>(ws_tx: &mut S, text: &str) -> Result<()>
where
    S: SinkExt<Message> + Unpin,
{
    ws_tx.send(Message::Text(text.to_string().into())).await.map_err(|_| anyhow::anyhow!("Send Error"))
}

fn msg_to_text(msg: Message) -> Option<String> {
    match msg {
        Message::Text(t) => Some(t.to_string()),
        Message::Binary(b) => String::from_utf8(b.to_vec()).ok(),
        _ => None,
    }
}
