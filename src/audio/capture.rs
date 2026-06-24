use anyhow::Result;
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use std::sync::{Arc, Mutex};
use opus::{Encoder, Application, Channels};

/// Starts capturing audio from the default microphone, encoding it to Opus,
/// and sending encoded packets over `audio_tx`.
///
/// Also sends a per-frame RMS level on `level_tx` so the caller can drive
/// speaking-state detection without decoding the Opus stream.
pub fn start_capture(
    audio_tx: tokio::sync::mpsc::UnboundedSender<Vec<u8>>,
    level_tx: tokio::sync::mpsc::UnboundedSender<f32>,
) -> Result<()> {
    std::thread::spawn(move || {
        let host   = cpal::default_host();
        let device = host.default_input_device().expect("No input device available");

        let config = cpal::StreamConfig {
            channels:    1,
            sample_rate: cpal::SampleRate(48000),
            buffer_size: cpal::BufferSize::Default,
        };

        let encoder = Encoder::new(48000, Channels::Mono, Application::Voip)
            .expect("Failed to create Opus encoder");

        let encoder      = Arc::new(Mutex::new(encoder));
        let frame_size   = 960; // 20 ms at 48 kHz
        let mut sample_buffer = Vec::with_capacity(frame_size);
        let mut out_buf  = vec![0u8; 4000];
        let err_fn = |err| log::error!("audio capture error: {}", err);

        let stream = device.build_input_stream(
            &config,
            move |data: &[f32], _: &cpal::InputCallbackInfo| {
                let mut enc = encoder.lock().unwrap();
                for &sample in data {
                    sample_buffer.push(sample);
                    if sample_buffer.len() == frame_size {
                        // RMS level for speaking detection (computed before encoding).
                        let rms = (sample_buffer.iter()
                            .map(|&s| (s as f64) * (s as f64))
                            .sum::<f64>()
                            / frame_size as f64)
                            .sqrt() as f32;
                        let _ = level_tx.send(rms);

                        if let Ok(len) = enc.encode_float(&sample_buffer, &mut out_buf) {
                            let _ = audio_tx.send(out_buf[..len].to_vec());
                        }
                        sample_buffer.clear();
                    }
                }
            },
            err_fn,
            None,
        ).expect("Failed to build input stream");

        stream.play().expect("Failed to play input stream");
        log::info!("Microphone capture started.");

        loop { std::thread::park(); }
    });

    Ok(())
}
