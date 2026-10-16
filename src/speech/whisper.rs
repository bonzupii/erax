//! Built-in STT: Whisper large-v3-turbo q5_0 via whisper-rs.
//!
//! The model bytes are embedded by build.rs (`include_bytes!` over the
//! `OUT_DIR` provisioning) and loaded from memory; no model file is
//! read from disk at runtime. The backend is chunked: each
//! `transcribe_stream` call is one caller-delineated chunk (e.g. a
//! silence-delimited span) and yields its `Final` directly. Partial
//! hypotheses are never emitted by this backend.

use super::provider::{
    LanguageCode, Pcm16k, RecognizeError, SpeechRecognizer, Transcript, TranscriptEvent,
};
use whisper_rs::{FullParams, SamplingStrategy, WhisperContext, WhisperContextParameters};

/// Embedded Whisper large-v3-turbo q5_0 weights, provisioned by
/// build.rs. Present only when the speech backend was enabled at
/// build time.
#[cfg(speech_provisioned)]
static WHISPER_MODEL: &[u8] = include_bytes!(concat!(
    env!("ERAX_SPEECH_WEIGHTS_DIR"),
    "/ggml-large-v3-turbo-q5_0.bin"
));

/// Whisper.cpp wants 16 kHz mono; anything else is a caller bug, not
/// a resampling request.
const WHISPER_SAMPLE_RATE: u32 = 16_000;

/// Maximum audio per `full` call: whisper.cpp's native 30 s window.
/// Longer utterances are transcribed window by window and the texts
/// concatenated; segment timestamps restart per window.
const WINDOW_SAMPLES: usize = 30 * WHISPER_SAMPLE_RATE as usize;

/// Built-in speech recognizer.
pub struct WhisperRecognizer {
    ctx: WhisperContext,
    language: Option<String>,
}

impl WhisperRecognizer {
    /// Load the embedded model. Fails typed when the weights were not
    /// provisioned (backend compiled without them).
    pub fn new() -> Result<Self, RecognizeError> {
        #[cfg(speech_provisioned)]
        {
            let ctx = WhisperContext::new_from_buffer_with_params(
                WHISPER_MODEL,
                WhisperContextParameters::default(),
            )
            .map_err(|e| RecognizeError::ModelLoad(format!("whisper init: {e:?}")))?;
            Ok(Self {
                ctx,
                language: None,
            })
        }
        #[cfg(not(speech_provisioned))]
        {
            Err(RecognizeError::ModelLoad(
                "whisper weights were not provisioned by build.rs".into(),
            ))
        }
    }

    fn full_params(&self) -> FullParams<'_, '_> {
        let mut params = FullParams::new(SamplingStrategy::Greedy { best_of: 1 });
        params.set_n_threads(4);
        params.set_translate(false);
        params.set_print_special(false);
        params.set_print_progress(false);
        params.set_print_realtime(false);
        params.set_print_timestamps(false);
        if let Some(lang) = self.language.as_deref() {
            params.set_language(Some(lang));
        }
        params
    }

    fn transcribe_window(&mut self, audio: &[f32]) -> Result<String, RecognizeError> {
        let mut state = self
            .ctx
            .create_state()
            .map_err(|e| RecognizeError::Inference(format!("whisper state: {e:?}")))?;
        state
            .full(self.full_params(), audio)
            .map_err(|e| RecognizeError::Inference(format!("whisper full: {e:?}")))?;
        let n = state.full_n_segments();
        let mut text = String::new();
        for i in 0..n {
            if let Some(seg) = state.get_segment(i) {
                if let Ok(s) = seg.to_str() {
                    text.push_str(s.trim());
                    text.push(' ');
                }
            }
        }
        Ok(text.trim().to_string())
    }
}

impl SpeechRecognizer for WhisperRecognizer {
    fn transcribe_stream(
        &mut self,
        audio: &[f32],
    ) -> Result<Vec<TranscriptEvent>, RecognizeError> {
        if audio.is_empty() {
            return Ok(Vec::new());
        }
        let t = self.transcribe_utterance(audio.to_vec())?;
        if t.text.is_empty() {
            return Ok(Vec::new());
        }
        Ok(vec![TranscriptEvent::Final {
            text: t.text,
            words: t.words,
        }])
    }

    fn transcribe_utterance(&mut self, audio: Pcm16k) -> Result<Transcript, RecognizeError> {
        if audio.is_empty() {
            return Err(RecognizeError::AudioFormat("empty audio".into()));
        }
        let mut text = String::new();
        for window in audio.chunks(WINDOW_SAMPLES) {
            let w = self.transcribe_window(window)?;
            if !w.is_empty() {
                if !text.is_empty() {
                    text.push(' ');
                }
                text.push_str(&w);
            }
        }
        // Word-level timestamps are not plumbed through whisper-rs's
        // safe API (they live in the FFI token structs); the words vec
        // stays empty rather than carrying interpolated fiction.
        Ok(Transcript {
            text,
            words: Vec::new(),
        })
    }

    fn set_language(&mut self, lang: Option<LanguageCode>) -> Result<(), RecognizeError> {
        self.language = lang.map(|l| l.0.to_string());
        Ok(())
    }
}
