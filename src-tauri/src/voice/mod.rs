//! Voice input for the Ask box: record a question and turn it into text on this computer.
//! Audio stays in memory, never touches disk, and is never sent anywhere.

pub mod audio;
pub mod model;
pub mod transcribe;

use crate::error::AppResult;
use audio::Recording;
use std::path::PathBuf;
use std::sync::atomic::AtomicBool;
use std::sync::{Arc, Mutex};
use whisper_rs::WhisperContext;

pub struct Voice {
    pub model_path: PathBuf,
    recording: Mutex<Option<Recording>>,
    /// The loaded model, kept between questions. Checked against its hash when first loaded.
    context: Mutex<Option<Arc<WhisperContext>>>,
    pub downloading: AtomicBool,
    pub cancel_download: AtomicBool,
}

impl Voice {
    pub fn new(data_dir: &std::path::Path) -> Self {
        Voice {
            model_path: model::model_path(data_dir),
            recording: Mutex::new(None),
            context: Mutex::new(None),
            downloading: AtomicBool::new(false),
            cancel_download: AtomicBool::new(false),
        }
    }

    /// Starts recording, replacing (and discarding) any recording already running.
    pub fn start(&self, on_event: impl Fn(audio::Event) + Send + 'static) -> AppResult<()> {
        if !model::is_downloaded(&self.model_path) {
            return Err("Download the voice model first.".into());
        }
        let mut slot = self.recording.lock().unwrap();
        slot.take();
        *slot = Some(Recording::start(on_event)?);
        Ok(())
    }

    /// Stops recording and returns the audio, or `None` when nothing was recording.
    pub fn stop(&self) -> Option<Recording> {
        self.recording.lock().unwrap().take()
    }

    /// The loaded model, loading (and verifying) it the first time. Slow: call off the UI thread.
    pub fn context(&self) -> AppResult<Arc<WhisperContext>> {
        let mut slot = self.context.lock().unwrap();
        if let Some(ctx) = slot.as_ref() {
            return Ok(ctx.clone());
        }
        if !model::is_downloaded(&self.model_path) {
            return Err("Download the voice model first.".into());
        }
        model::verify(&self.model_path, model::MODEL_SHA256)?;
        let ctx = Arc::new(transcribe::load(&self.model_path)?);
        *slot = Some(ctx.clone());
        Ok(ctx)
    }

    /// Removes the model from disk and memory.
    pub fn remove_model(&self) -> AppResult<()> {
        self.recording.lock().unwrap().take();
        self.context.lock().unwrap().take();
        model::remove(&self.model_path)
    }
}
