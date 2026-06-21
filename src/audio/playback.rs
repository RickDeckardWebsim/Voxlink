use anyhow::{Context, Result};
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use ringbuf::{HeapRb, traits::{Consumer as _, Producer as _, Split, Observer}};
use std::sync::{Arc, Mutex};
use opus::{Decoder, Channels};
use std::thread;

/// Starts the audio playback stream and a dedicated decoder thread.
/// Returns the sender where Opus packets should be sent.
pub fn start_playback() -> Result<tokio::sync::mpsc::UnboundedSender<Vec<u8>>> {
    let (packet_tx, mut packet_rx) = tokio::sync::mpsc::unbounded_channel::<Vec<u8>>();

    std::thread::spawn(move || {
        let host = cpal::default_host();
        let device = host.default_output_device().expect("No output device available");
        
        let config = cpal::StreamConfig {
            channels: 1,
            sample_rate: cpal::SampleRate(48000),
            buffer_size: cpal::BufferSize::Default,
        };

        let rb = HeapRb::<f32>::new(48000);
        let (mut prod, mut cons) = rb.split();

        let err_fn = |err| log::error!("an error occurred on playback stream: {}", err);

        let stream = device.build_output_stream(
            &config,
            move |data: &mut [f32], _: &cpal::OutputCallbackInfo| {
                for sample in data.iter_mut() {
                    *sample = cons.try_pop().unwrap_or(0.0);
                }
            },
            err_fn,
            None,
        ).expect("Failed to build output stream");

        stream.play().expect("Failed to play output stream");
        log::info!("Speaker playback started.");

        // We can just use THIS thread as the Opus decoder thread since we are parked anyway!
        let mut decoder = Decoder::new(48000, Channels::Mono).expect("Failed to create Opus decoder");
        let mut decode_buf = vec![0f32; 5760]; // Max Opus frame size

        while let Some(packet) = packet_rx.blocking_recv() {
            if let Ok(len) = decoder.decode_float(&packet, &mut decode_buf, false) {
                for &sample in &decode_buf[..len] {
                    while prod.is_full() {
                        std::thread::yield_now();
                    }
                    let _ = prod.try_push(sample);
                }
            }
        }
        
        // Loop will exit if the channel is dropped, so we implicitly drop the stream then.
    });

    Ok(packet_tx)
}
