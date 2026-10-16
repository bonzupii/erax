//! Generic speech backend: STT and TTS provider traits.
//!
//! The built-in providers (Whisper STT, Kokoro TTS) implement these
//! traits; a user wires an alternative by implementing the same two
//! traits and selecting `SpeechBackend::Custom`. The traits are
//! shaped by how real engines differ (chunked vs streaming, voiced
//! vs voiceless), so every axis of variation is typed, not a config
//! hack.

use super::VoiceId;

/// 16 kHz mono `f32` PCM: the interface sample rate. Every candidate
/// surveyed (whisper.cpp, sherpa-onnx, Moonshine, floravox) speaks
/// this format; providers that produce another rate resample
/// internally, so the interface never sees anything else.
pub type Pcm16k = Vec<f32>;

/// Sample rate of [`Pcm16k`], in Hz.
pub const PCM16K_SAMPLE_RATE: u32 = 16_000;

/// Word-level timing inside a finalized transcript.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct WordTiming {
    /// Start of the byte range of the word in the final text.
    pub start_byte: usize,
    /// End of the byte range of the word in the final text.
    pub end_byte: usize,
    /// Word start in seconds from the utterance start.
    pub start_secs: f32,
    /// Word end in seconds from the utterance start.
    pub end_secs: f32,
}

/// One STT hypothesis event.
#[derive(Debug, Clone, PartialEq)]
pub enum TranscriptEvent {
    /// A partial hypothesis. `stable_prefix_len` is the byte count the
    /// engine commits to not revising (LocalAgreement semantics).
    /// Chunked engines never emit partials.
    Partial {
        /// Current hypothesis text.
        text: String,
        /// Committed prefix length in bytes.
        stable_prefix_len: usize,
    },
    /// A finalized segment, with word timings where the engine
    /// provides them.
    Final {
        /// Final transcript text.
        text: String,
        /// Word timings, possibly empty.
        words: Vec<WordTiming>,
    },
}

/// A fully transcribed utterance.
#[derive(Debug, Clone, PartialEq)]
pub struct Transcript {
    /// Transcript text.
    pub text: String,
    /// Word timings, possibly empty.
    pub words: Vec<WordTiming>,
}

/// ISO 639-1 language code for STT language selection.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LanguageCode(pub &'static str);

impl LanguageCode {
    /// English.
    pub const EN: LanguageCode = LanguageCode("en");
}

/// STT: one trait covers chunked and streaming engines.
///
/// Chunked engines (whisper.cpp VAD-gated) emit nothing until a
/// chunk boundary, then a `Final`; streaming engines (Moonshine)
/// emit `Partial`s with bounded latency. Consumers handle both.
pub trait SpeechRecognizer: Send {
    /// Feed audio; receive hypotheses. The caller delineates chunks
    /// (e.g. on silence); the backend treats each call as one chunk
    /// and returns its `Final`. Returns an empty vec only when the
    /// chunk held no speech. Inference failures are typed, never
    /// swallowed: a streaming consumer that cannot handle the error
    /// is a consumer bug.
    fn transcribe_stream(&mut self, audio: &[f32]) -> Result<Vec<TranscriptEvent>, RecognizeError>;

    /// Transcribe one complete utterance. An empty utterance yields
    /// an empty transcript, not an error: empty input is a degenerate
    /// call everywhere in this trait (see `transcribe_stream` and
    /// `SpeechSynthesizer::synthesize`).
    fn transcribe_utterance(&mut self, audio: Pcm16k) -> Result<Transcript, RecognizeError>;

    /// Language selection. `None` is the engine default (auto-detect
    /// where supported). Engines without language selection return
    /// `Ok(())` and ignore the argument.
    fn set_language(&mut self, lang: Option<LanguageCode>) -> Result<(), RecognizeError>;
}

/// Synthesized audio: 16 kHz mono `f32` PCM, in emission order.
#[derive(Debug, Clone, PartialEq)]
pub struct SynthesisStream {
    /// Audio chunks in emission order. Chunked engines emit one chunk
    /// per sentence; streaming engines emit many small chunks.
    pub chunks: Vec<Pcm16k>,
    /// Milliseconds from call to first audio, as measured by the
    /// provider.
    pub first_audio_latency_ms: u64,
}

impl SynthesisStream {
    /// The whole utterance as one PCM buffer.
    pub fn concat(&self) -> Pcm16k {
        self.chunks.concat()
    }

    /// Total sample count across chunks.
    pub fn len_samples(&self) -> usize {
        self.chunks.iter().map(Vec::len).sum()
    }
}

/// TTS: one trait covers chunked and streaming synthesizers.
pub trait SpeechSynthesizer: Send {
    /// Synthesize text to 16 kHz mono PCM. Empty or whitespace-only
    /// input yields an empty stream, not an error.
    fn synthesize(&mut self, text: &str) -> Result<SynthesisStream, SynthesizeError>;

    /// Voice selection. Voiceless models return
    /// `SynthesizeError::Unsupported`.
    fn set_voice(&mut self, voice: VoiceId) -> Result<(), SynthesizeError>;
}

/// STT failures: genuine failures only (model load, bad audio,
/// inference failure, cancellation). Never silently swallowed; the
/// infallibility law forbids defensive fallbacks around them.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RecognizeError {
    /// The speech backend is compiled out (`SpeechBackend::Off`).
    Disabled,
    /// The model failed to load (corrupt weights, missing backend).
    ModelLoad(String),
    /// The audio did not meet the interface contract.
    AudioFormat(String),
    /// Inference itself failed.
    Inference(String),
    /// Transcription was cancelled.
    Cancelled,
}

/// TTS failures: genuine failures only, same contract as
/// [`RecognizeError`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SynthesizeError {
    /// The speech backend is compiled out (`SpeechBackend::Off`).
    Disabled,
    /// The model failed to load.
    ModelLoad(String),
    /// Text could not be converted to phonemes.
    Phonemize(String),
    /// Inference itself failed.
    Inference(String),
    /// The voice is not supported by this provider.
    Unsupported,
    /// Synthesis was cancelled.
    Cancelled,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn synthesis_stream_concat_preserves_order() {
        let s = SynthesisStream {
            chunks: vec![vec![1.0, 2.0], vec![3.0]],
            first_audio_latency_ms: 10,
        };
        assert_eq!(s.concat(), vec![1.0, 2.0, 3.0]);
        assert_eq!(s.len_samples(), 3);
    }
}
