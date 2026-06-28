// ─────────────────────────────────────────────────────────────────────────────
// audio/notification.rs — One-shot notification ping for @mentions
//
// Fire-and-forget: `play_notification` spawns a thread, decodes the bundled
// WAV via hound, plays it once through cpal's default output device, then
// drops the stream. Errors are logged and NEVER propagated — this must not
// block or crash the UI thread.
// ─────────────────────────────────────────────────────────────────────────────

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};

const NOTIFICATION_WAV: &[u8] = include_bytes!("assets/notification.wav");

/// Play a short notification ping on the default output device.
/// Fire-and-forget; errors are logged and never propagated (must not block the UI).
pub fn play_notification() {
    std::thread::spawn(|| {
        if let Err(e) = play_notification_blocking() {
            log::warn!("[audio] notification playback failed: {}", e);
        }
    });
}

fn play_notification_blocking() -> anyhow::Result<()> {
    // Decode the WAV using hound
    let cursor = std::io::Cursor::new(NOTIFICATION_WAV);
    let reader = hound::WavReader::new(cursor)?;
    let spec = reader.spec();
    let sample_rate = spec.sample_rate; // hound 3.5: WavSpec.sample_rate is u32
    let channels = spec.channels;

    // Collect samples as f32, normalized to [-1.0, 1.0].
    // The bundled WAV is 16-bit PCM. hound sign-extends 16-bit samples into
    // the i16 range (±32767) — it does NOT scale to fill i32 — so we must read
    // as i16 and divide by i16::MAX. Dividing by i32::MAX would yield ~1.5e-5
    // (effectively silent).
    let samples: Vec<f32> = match spec.sample_format {
        hound::SampleFormat::Int => {
            reader.into_samples::<i16>().filter_map(|s| s.ok()).map(|s| s as f32 / i16::MAX as f32).collect()
        }
        hound::SampleFormat::Float => {
            reader.into_samples::<f32>().filter_map(|s| s.ok()).collect()
        }
    };

    // Set up cpal output (match the WAV's sample rate; downmix to mono if needed)
    let host = cpal::default_host();
    let device = host.default_output_device().ok_or_else(|| anyhow::anyhow!("No output device"))?;
    let config = cpal::StreamConfig {
        channels: 1, // mono output
        sample_rate: cpal::SampleRate(sample_rate),
        buffer_size: cpal::BufferSize::Default,
    };

    // If the WAV is stereo, downmix to mono by averaging channels
    let mono_samples: Vec<f32> = if channels == 2 {
        samples.chunks(2).map(|pair| (pair[0] + pair.get(1).copied().unwrap_or(0.0)) / 2.0).collect()
    } else {
        samples
    };

    let samples_arc = std::sync::Arc::new(mono_samples);
    let pos = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let samples_clone = samples_arc.clone();
    let pos_clone = pos.clone();

    let err_fn = |err| log::error!("[audio] notification stream error: {}", err);
    let stream = device.build_output_stream(
        &config,
        move |data: &mut [f32], _: &cpal::OutputCallbackInfo| {
            for sample in data.iter_mut() {
                let i = pos_clone.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                *sample = if i < samples_clone.len() { samples_clone[i] } else { 0.0 };
            }
        },
        err_fn,
        None,
    )?;
    stream.play()?;

    // Sleep for the duration of the sound, then drop the stream
    let duration_secs = samples_arc.len() as f32 / sample_rate as f32;
    std::thread::sleep(std::time::Duration::from_secs_f32(duration_secs + 0.1));
    // stream is dropped here, stopping playback
    drop(stream);
    Ok(())
}
