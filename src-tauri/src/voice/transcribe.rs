//! Speech to text with whisper.cpp, entirely on this computer.

use crate::error::{AppError, AppResult};
use std::path::Path;
use whisper_rs::{FullParams, SamplingStrategy, WhisperContext, WhisperContextParameters};

/// Under this much audio there's nothing worth transcribing (a click of the button).
const MIN_SAMPLES: usize = 16_000 / 4;
/// Whisper reads at most about 224 tokens of prompt; this many characters stays under that.
const MAX_PROMPT_CHARS: usize = 600;

/// Loads the model. Takes a moment, so the loaded model is kept for the next question.
pub fn load(path: &Path) -> AppResult<WhisperContext> {
    whisper_rs::install_logging_hooks();
    let path = path.to_str().ok_or("The voice model's path isn't valid text.")?;
    WhisperContext::new_with_params(path, WhisperContextParameters::default())
        .map_err(|e| AppError::User(format!("Could not load the voice model: {e}")))
}

/// Transcribes 16 kHz mono audio. `prompt` (table and column names) nudges Whisper toward
/// spelling those words the way the schema does.
pub fn transcribe(ctx: &WhisperContext, audio: &[f32], prompt: &str) -> AppResult<String> {
    if audio.len() < MIN_SAMPLES {
        return Ok(String::new());
    }
    let mut state = ctx.create_state().map_err(|e| AppError::User(format!("Could not start transcribing: {e}")))?;
    let mut params = FullParams::new(SamplingStrategy::Greedy { best_of: 1 });
    let threads = std::thread::available_parallelism().map(|n| n.get()).unwrap_or(4).min(8);
    params.set_n_threads(threads as i32);
    params.set_language(Some("en"));
    params.set_no_context(true);
    params.set_no_timestamps(true);
    params.set_suppress_blank(true);
    params.set_suppress_nst(true);
    params.set_print_special(false);
    params.set_print_progress(false);
    params.set_print_realtime(false);
    params.set_print_timestamps(false);
    let prompt = clip_prompt(prompt);
    if !prompt.is_empty() {
        params.set_initial_prompt(prompt);
    }
    state.full(params, audio).map_err(|e| AppError::User(format!("Transcription failed: {e}")))?;
    let text: Vec<String> = state.as_iter().filter_map(|s| s.to_str_lossy().ok().map(|t| t.into_owned())).collect();
    Ok(clean(&text.join(" ")))
}

/// The prompt cut at a word boundary to fit Whisper's limit.
fn clip_prompt(prompt: &str) -> &str {
    let prompt = prompt.trim();
    if prompt.len() <= MAX_PROMPT_CHARS {
        return prompt;
    }
    let mut end = MAX_PROMPT_CHARS;
    while !prompt.is_char_boundary(end) {
        end -= 1;
    }
    let cut = prompt[..end].rfind([' ', ',']).map_or(&prompt[..end], |i| &prompt[..i]);
    cut.trim_end_matches([' ', ','])
}

/// Drops Whisper's non-speech markers like `[BLANK_AUDIO]` or `(keyboard clicking)` and
/// squeezes whitespace.
pub fn clean(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut depth = 0usize;
    for c in text.chars() {
        match c {
            '[' | '(' => depth += 1,
            ']' | ')' if depth > 0 => depth -= 1,
            _ if depth == 0 => out.push(c),
            _ => {}
        }
    }
    out.split_whitespace().collect::<Vec<_>>().join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clean_drops_non_speech_markers() {
        assert_eq!(clean(" [BLANK_AUDIO]"), "");
        assert_eq!(clean(" Top ten customers (keyboard clicking)  by revenue."), "Top ten customers by revenue.");
        assert_eq!(clean("  orders\n from  last week "), "orders from last week");
    }

    #[test]
    fn long_prompts_are_cut_at_a_word() {
        let long = "order_items, ".repeat(100);
        let clipped = clip_prompt(&long);
        assert!(clipped.len() <= MAX_PROMPT_CHARS);
        assert!(clipped.ends_with("order_items"));
        assert_eq!(clip_prompt(" customers, orders "), "customers, orders");
    }

    /// A real transcription through the same path the app uses. Needs the downloaded model:
    /// `SAYQL_WHISPER_MODEL=/path/to/ggml-base.en-q8_0.bin cargo test --lib -- --ignored transcribes`
    #[test]
    #[ignore]
    fn transcribes_the_test_clip() {
        let model = std::env::var("SAYQL_WHISPER_MODEL").expect("set SAYQL_WHISPER_MODEL");
        super::super::model::verify(Path::new(&model), super::super::model::MODEL_SHA256).unwrap();
        let ctx = load(Path::new(&model)).unwrap();
        let audio = read_wav(include_bytes!("testdata/top-customers.wav"));
        let text = transcribe(&ctx, &audio, "customers, orders, order_items, total_spend").unwrap().to_lowercase();
        println!("transcript: {text}");
        assert!(text.contains("customers"), "{text}");
        assert!(text.contains("spend") || text.contains("spent"), "{text}");
    }

    /// Reads a 16-bit PCM mono WAV into floats.
    pub fn read_wav(bytes: &[u8]) -> Vec<f32> {
        let mut i = 12;
        while i + 8 <= bytes.len() {
            let id = &bytes[i..i + 4];
            let len = u32::from_le_bytes(bytes[i + 4..i + 8].try_into().unwrap()) as usize;
            if id == b"data" {
                return bytes[i + 8..(i + 8 + len).min(bytes.len())]
                    .chunks_exact(2)
                    .map(|b| i16::from_le_bytes([b[0], b[1]]) as f32 / 32768.0)
                    .collect();
            }
            i += 8 + len + (len & 1);
        }
        panic!("no data chunk");
    }

    #[test]
    fn the_test_clip_is_16k_mono_speech() {
        let bytes = include_bytes!("testdata/top-customers.wav");
        assert_eq!(u16::from_le_bytes([bytes[22], bytes[23]]), 1, "mono");
        assert_eq!(u32::from_le_bytes(bytes[24..28].try_into().unwrap()), 16_000);
        let audio = read_wav(bytes);
        assert!(audio.len() > MIN_SAMPLES);
        assert!(audio.iter().any(|s| s.abs() > 0.05));
    }
}
