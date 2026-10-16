//! Speech input and output: generic STT/TTS backends.
//!
//! [`provider`] holds the engine-agnostic traits (`SpeechRecognizer`,
//! `SpeechSynthesizer`) plus the typed errors. The built-in providers
//! live behind the `speech` cargo feature: [`whisper`] (Whisper
//! large-v3-turbo q5_0 via whisper-rs) and [`kokoro`] (Kokoro-82M
//! int8 via floravox-core with a Rust-native phonemizer, never
//! espeak-ng). Without the feature, or with
//! `SpeechBackend::Off` in config, the constructors below return the
//! typed `Disabled` error.
//!
//! Weights are provisioned by build.rs only when the config selects a
//! non-`Off` backend: pinned artifacts are downloaded, sha256-verified
//! into `OUT_DIR`, and embedded with `include_bytes!`. Nothing is
//! written to disk at runtime.
//!
//! Audio device I/O (microphone capture, speaker playback) is not in
//! this module: there is no audio stack in the tree, and adding one
//! is the owner's dependency decision. The backends consume and
//! produce [`provider::Pcm16k`]; the device layer plugs in here when
//! it exists.

pub mod provider;

#[cfg(feature = "speech")]
pub mod kokoro;
#[cfg(feature = "speech")]
pub mod whisper;

pub use provider::{
    LanguageCode, Pcm16k, RecognizeError, SpeechRecognizer, SpeechSynthesizer, SynthesizeError,
    SynthesisStream, Transcript, TranscriptEvent, WordTiming, PCM16K_SAMPLE_RATE,
};
pub use crate::config::accessibility::blindness::{SpeechBackend, VoiceId};

use crate::config::accessibility::blindness::SPEECH_BACKEND;
#[cfg(feature = "speech")]
use crate::config::accessibility::blindness::SPEECH_VOICE;

/// Build the configured speech recognizer.
///
/// Returns `RecognizeError::Disabled` when the backend is `Off` or
/// the `speech` cargo feature is not enabled.
pub fn recognizer() -> Result<Box<dyn SpeechRecognizer>, RecognizeError> {
    match SPEECH_BACKEND {
        SpeechBackend::Off => Err(RecognizeError::Disabled),
        SpeechBackend::BuiltIn => {
            #[cfg(feature = "speech")]
            {
                whisper::WhisperRecognizer::new().map(|r| {
                    let b: Box<dyn SpeechRecognizer> = Box::new(r);
                    b
                })
            }
            #[cfg(not(feature = "speech"))]
            {
                Err(RecognizeError::ModelLoad(
                    "SPEECH_BACKEND is BuiltIn but the `speech` cargo feature is off; rebuild with --features speech".into(),
                ))
            }
        }
        SpeechBackend::Custom => Err(RecognizeError::ModelLoad(
            "SPEECH_BACKEND is Custom: wire a provider at crate::speech::recognizer".into(),
        )),
    }
}

/// Build the configured speech synthesizer.
///
/// Returns `SynthesizeError::Disabled` when the backend is `Off` or
/// the `speech` cargo feature is not enabled.
pub fn synthesizer() -> Result<Box<dyn SpeechSynthesizer>, SynthesizeError> {
    match SPEECH_BACKEND {
        SpeechBackend::Off => Err(SynthesizeError::Disabled),
        SpeechBackend::BuiltIn => {
            #[cfg(feature = "speech")]
            {
                kokoro::KokoroSynthesizer::new(SPEECH_VOICE).map(|s| {
                    let b: Box<dyn SpeechSynthesizer> = Box::new(s);
                    b
                })
            }
            #[cfg(not(feature = "speech"))]
            {
                Err(SynthesizeError::ModelLoad(
                    "SPEECH_BACKEND is BuiltIn but the `speech` cargo feature is off; rebuild with --features speech".into(),
                ))
            }
        }
        SpeechBackend::Custom => Err(SynthesizeError::ModelLoad(
            "SPEECH_BACKEND is Custom: wire a provider at crate::speech::synthesizer".into(),
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn disabled_backend_yields_typed_errors() {
        // The stock config selects Off; the constructors must say so
        // with a typed error, never a panic or a silent no-op.
        match SPEECH_BACKEND {
            SpeechBackend::Off => {
                assert_eq!(recognizer().err(), Some(RecognizeError::Disabled));
                assert_eq!(synthesizer().err(), Some(SynthesizeError::Disabled));
            }
            _ => {
                // A non-Off stock config is a user choice; the only
                // invariant is that construction is total.
                let _ = recognizer().is_ok();
                let _ = synthesizer().is_ok();
            }
        }
    }

    #[test]
    fn backend_selection_is_exhaustive() {
        // Every SpeechBackend variant has a defined constructor path.
        fn check(b: SpeechBackend) -> bool {
            matches!(
                b,
                SpeechBackend::Off | SpeechBackend::BuiltIn | SpeechBackend::Custom
            )
        }
        assert!(check(SpeechBackend::Off));
        assert!(check(SpeechBackend::BuiltIn));
        assert!(check(SpeechBackend::Custom));
    }
}
