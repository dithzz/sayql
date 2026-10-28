//! Microphone capture with cpal, turned into the 16 kHz mono audio Whisper expects.
//!
//! Capture runs in Rust rather than through the webview's getUserMedia: WebKitGTK on Linux
//! ships with media capture turned off, and WKWebView and WebView2 each add their own
//! permission prompt on top of the operating system's. cpal talks to Core Audio, WASAPI and
//! ALSA directly, so the only prompt is the system's own, and the audio never passes
//! through the web layer.

use crate::error::{AppError, AppResult};
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{ErrorKind, FromSample, Sample, SampleFormat, SizedSample};
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicUsize, Ordering};
use std::sync::mpsc::{self, RecvTimeoutError};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::Duration;

/// Whisper's sample rate.
pub const WHISPER_RATE: u32 = 16_000;
/// Longer recordings stop adding audio; a question rarely takes more than a few seconds.
pub const MAX_SECONDS: usize = 120;

pub const NO_MIC: &str = "No microphone found.";
pub const MIC_DENIED: &str = if cfg!(target_os = "macos") {
    "SayQL can't use the microphone. Allow it in System Settings → Privacy & Security → Microphone."
} else if cfg!(windows) {
    "SayQL can't use the microphone. Allow it in Settings → Privacy & security → Microphone."
} else {
    "SayQL can't use the microphone. Check your sound settings."
};

/// What a recording reports while it runs.
pub enum Event {
    /// Loudness of the last moment, 0–1, for the level meter.
    Level(f32),
    /// A full second of exact digital silence: the system is blocking the microphone.
    /// macOS does this, rather than failing, when access was denied.
    Blocked,
}

/// A recording in progress. Stop it to get the audio.
pub struct Recording {
    stop: mpsc::Sender<()>,
    thread: Option<JoinHandle<Captured>>,
}

struct Captured {
    samples: Vec<f32>,
    rate: u32,
    peak: f32,
}

/// What the audio callback shares with the recording thread.
struct Shared {
    samples: Mutex<Vec<f32>>,
    /// Latest RMS level and loudest sample so far, as f32 bits.
    level: AtomicU32,
    peak: AtomicU32,
    frames: AtomicUsize,
    failed: AtomicBool,
}

impl Recording {
    /// Opens the default microphone and starts recording. `on_event` is called from a
    /// background thread about 20 times a second.
    pub fn start(on_event: impl Fn(Event) + Send + 'static) -> AppResult<Recording> {
        let (stop, stop_rx) = mpsc::channel();
        let (ready_tx, ready_rx) = mpsc::channel();
        let thread = std::thread::spawn(move || {
            let shared = Arc::new(Shared {
                samples: Mutex::new(Vec::new()),
                level: AtomicU32::new(0),
                peak: AtomicU32::new(0),
                frames: AtomicUsize::new(0),
                failed: AtomicBool::new(false),
            });
            // cpal streams can't move between threads on every platform, so this one lives here.
            let (stream, rate) = match open_stream(shared.clone()) {
                Ok(s) => s,
                Err(e) => {
                    let _ = ready_tx.send(Err(e));
                    return Captured {
                        samples: Vec::new(),
                        rate: WHISPER_RATE,
                        peak: 0.0,
                    };
                }
            };
            let _ = ready_tx.send(Ok(()));
            let mut warned = false;
            while let Err(RecvTimeoutError::Timeout) =
                stop_rx.recv_timeout(Duration::from_millis(50))
            {
                on_event(Event::Level(f32::from_bits(
                    shared.level.load(Ordering::Relaxed),
                )));
                let silent = f32::from_bits(shared.peak.load(Ordering::Relaxed)) == 0.0;
                if !warned && silent && shared.frames.load(Ordering::Relaxed) >= rate as usize {
                    warned = true;
                    on_event(Event::Blocked);
                }
                if shared.failed.load(Ordering::Relaxed) {
                    break;
                }
            }
            drop(stream);
            let samples = std::mem::take(&mut *shared.samples.lock().unwrap());
            Captured {
                samples,
                rate,
                peak: f32::from_bits(shared.peak.load(Ordering::Relaxed)),
            }
        });
        match ready_rx.recv() {
            Ok(Ok(())) => Ok(Recording {
                stop,
                thread: Some(thread),
            }),
            Ok(Err(e)) => Err(e),
            Err(_) => Err("Could not start recording.".into()),
        }
    }

    /// Stops recording and returns the audio as 16 kHz mono.
    pub fn finish(mut self) -> AppResult<Vec<f32>> {
        let captured = self.join();
        if captured.peak == 0.0 {
            return Err(MIC_DENIED.into());
        }
        Ok(resample(&captured.samples, captured.rate, WHISPER_RATE))
    }

    fn join(&mut self) -> Captured {
        let _ = self.stop.send(());
        self.thread
            .take()
            .and_then(|t| t.join().ok())
            .unwrap_or(Captured {
                samples: Vec::new(),
                rate: WHISPER_RATE,
                peak: 0.0,
            })
    }
}

impl Drop for Recording {
    /// Dropping a recording (canceling it) releases the microphone.
    fn drop(&mut self) {
        if self.thread.is_some() {
            self.join();
        }
    }
}

fn mic_error(e: cpal::Error) -> AppError {
    match e.kind() {
        ErrorKind::PermissionDenied => MIC_DENIED.into(),
        ErrorKind::DeviceNotAvailable | ErrorKind::HostUnavailable => NO_MIC.into(),
        _ => AppError::User(format!("Could not use the microphone: {e}")),
    }
}

fn open_stream(shared: Arc<Shared>) -> AppResult<(cpal::Stream, u32)> {
    let device = cpal::default_host().default_input_device().ok_or(NO_MIC)?;
    let supported = device.default_input_config().map_err(mic_error)?;
    let config = supported.config();
    let rate = config.sample_rate;
    let channels = config.channels as usize;
    let stream = match supported.sample_format() {
        SampleFormat::F32 => build::<f32>(&device, &config, channels, rate, shared),
        SampleFormat::I16 => build::<i16>(&device, &config, channels, rate, shared),
        SampleFormat::I32 => build::<i32>(&device, &config, channels, rate, shared),
        SampleFormat::U16 => build::<u16>(&device, &config, channels, rate, shared),
        SampleFormat::I8 => build::<i8>(&device, &config, channels, rate, shared),
        SampleFormat::U8 => build::<u8>(&device, &config, channels, rate, shared),
        SampleFormat::F64 => build::<f64>(&device, &config, channels, rate, shared),
        other => {
            return Err(AppError::User(format!(
                "Unsupported microphone format: {other}"
            )));
        }
    }?;
    stream.play().map_err(mic_error)?;
    Ok((stream, rate))
}

fn build<T>(
    device: &cpal::Device,
    config: &cpal::StreamConfig,
    channels: usize,
    rate: u32,
    shared: Arc<Shared>,
) -> AppResult<cpal::Stream>
where
    T: SizedSample,
    f32: FromSample<T>,
{
    let max = rate as usize * MAX_SECONDS;
    let on_error = shared.clone();
    device
        .build_input_stream(
            *config,
            move |data: &[T], _: &cpal::InputCallbackInfo| {
                let mono = to_mono(data, channels);
                let (mut sum, mut peak) =
                    (0.0f32, f32::from_bits(shared.peak.load(Ordering::Relaxed)));
                for s in &mono {
                    sum += s * s;
                    peak = peak.max(s.abs());
                }
                if !mono.is_empty() {
                    shared.level.store(
                        (sum / mono.len() as f32).sqrt().min(1.0).to_bits(),
                        Ordering::Relaxed,
                    );
                }
                shared.peak.store(peak.to_bits(), Ordering::Relaxed);
                shared.frames.fetch_add(mono.len(), Ordering::Relaxed);
                let mut samples = shared.samples.lock().unwrap();
                let room = max.saturating_sub(samples.len());
                samples.extend_from_slice(&mono[..mono.len().min(room)]);
            },
            move |_| on_error.failed.store(true, Ordering::Relaxed),
            None,
        )
        .map_err(mic_error)
}

/// Averages interleaved channels into one.
pub fn to_mono<T>(data: &[T], channels: usize) -> Vec<f32>
where
    T: Sample,
    f32: FromSample<T>,
{
    let channels = channels.max(1);
    data.chunks(channels)
        .map(|frame| frame.iter().map(|s| f32::from_sample(*s)).sum::<f32>() / frame.len() as f32)
        .collect()
}

/// Converts mono audio between sample rates. Downsampling averages each output sample's
/// span of input (a box filter, so higher frequencies don't fold back as noise); upsampling
/// interpolates linearly.
pub fn resample(input: &[f32], from: u32, to: u32) -> Vec<f32> {
    if from == to || input.is_empty() {
        return input.to_vec();
    }
    let ratio = from as f64 / to as f64;
    let len = ((input.len() as f64) / ratio).floor() as usize;
    (0..len)
        .map(|i| {
            let start = i as f64 * ratio;
            if ratio > 1.0 {
                let (a, b) = (
                    start as usize,
                    ((start + ratio) as usize)
                        .min(input.len())
                        .max(start as usize + 1),
                );
                input[a..b].iter().sum::<f32>() / (b - a) as f32
            } else {
                let a = start as usize;
                let b = (a + 1).min(input.len() - 1);
                let t = (start - a as f64) as f32;
                input[a] * (1.0 - t) + input[b] * t
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stereo_is_averaged_to_mono() {
        assert_eq!(to_mono(&[0.5f32, -0.5, 1.0, 0.0], 2), vec![0.0, 0.5]);
        assert_eq!(to_mono(&[i16::MAX, i16::MAX], 1).len(), 2);
        assert!((to_mono(&[i16::MIN], 1)[0] + 1.0).abs() < 1e-6);
    }

    #[test]
    fn resampling_48k_to_16k_keeps_duration_and_level() {
        let input = vec![0.25f32; 48_000];
        let out = resample(&input, 48_000, WHISPER_RATE);
        assert_eq!(out.len(), 16_000);
        assert!(out.iter().all(|s| (s - 0.25).abs() < 1e-6));
    }

    #[test]
    fn resampling_odd_rates_keeps_duration() {
        let out = resample(&vec![0.1f32; 44_100 * 2], 44_100, WHISPER_RATE);
        assert_eq!(out.len(), 32_000);
        let up = resample(&vec![0.1f32; 8_000], 8_000, WHISPER_RATE);
        assert_eq!(up.len(), 16_000);
        assert!(up.iter().all(|s| (s - 0.1).abs() < 1e-6));
    }

    #[test]
    fn downsampling_filters_out_high_frequencies() {
        // A tone at 24 kHz (alternating samples at 48 kHz) can't exist at 16 kHz; it must not alias.
        let input: Vec<f32> = (0..48_000)
            .map(|i| if i % 2 == 0 { 0.5 } else { -0.5 })
            .collect();
        let out = resample(&input, 48_000, WHISPER_RATE);
        let rms = (out.iter().map(|s| s * s).sum::<f32>() / out.len() as f32).sqrt();
        assert!(rms < 0.2, "rms {rms}");
    }

    #[test]
    fn same_rate_is_unchanged() {
        assert_eq!(
            resample(&[0.1, 0.2], WHISPER_RATE, WHISPER_RATE),
            vec![0.1, 0.2]
        );
        assert!(resample(&[], 48_000, WHISPER_RATE).is_empty());
    }
}
