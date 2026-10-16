//! Non-visual operation settings: typed `pub const`s.
//!
//! Announcements for screen-reader live regions, audio cues that
//! replace visual signals, and the speech backend (voice input and
//! spoken output). All off or silent by default; enabling them never
//! changes what a sighted user sees.

/// Announce the cursor's line number (not the column) to the web
/// shell's live region when it changes. Columns are never announced:
/// per-keystroke column chatter is the failure mode this disables.
pub const ANNOUNCE_CURSOR_LINE: bool = true;

/// Ring the terminal bell (`\x07`) on error-class cues when
/// [`super::CUES_ENABLED`] is true. The bell never fires for mode
/// changes or informational cues; those are visual or silent. This is
/// the non-visual error signal; its visual equivalent for deaf users
/// is [`super::hearing::CUE_VISUAL_FLASH`].
pub const CUE_ERROR_BELL: bool = true;

/// Blindness profile master. When true, the accessibility cue bus
/// ([`super::CUES_ENABLED`]) goes live for the non-visual cue senses
/// in this module. This is the constant the `blind` accessibility
/// profile flips; it exists so profiles compose by class without
/// sharing patch ground.
pub const BLIND_PROFILE: bool = false;

/// Which speech backend the editor wires in for voice input (STT)
/// and spoken output (TTS).
///
/// `Off` (the default) compiles the speech modules to disabled
/// stubs and provisions no weights. `BuiltIn` selects the
/// provisioned models below, fetched by build.rs only when this
/// constant is not `Off`. `Custom` is the user-provided provider
/// behind the generic traits in `crate::speech`: implement
/// `SpeechRecognizer`/`SpeechSynthesizer` and wire the constructors
/// at the dispatch point in `crate::speech`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SpeechBackend {
    /// No speech support.
    Off,
    /// Whisper large-v3-turbo q5_0 (STT) and Kokoro-82M int8 (TTS).
    BuiltIn,
    /// User-implemented provider behind the generic traits.
    Custom,
}

/// Speech backend selection. Off by default: enabling it pulls
/// ~690 MiB of weights at build time and links the inference
/// runtimes.
pub const SPEECH_BACKEND: SpeechBackend = SpeechBackend::Off;

/// Kokoro voice for the built-in TTS, one variant per provisioned
/// voice file. The default is the upstream default voice.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum VoiceId {
    Af,
    AfAlloy,
    AfAoede,
    AfBella,
    AfHeart,
    AfJessica,
    AfKore,
    AfNicole,
    AfNova,
    AfRiver,
    AfSarah,
    AfSky,
    AmAdam,
    AmEcho,
    AmEric,
    AmFenrir,
    AmLiam,
    AmMichael,
    AmOnyx,
    AmPuck,
    AmSanta,
    BfAlice,
    BfEmma,
    BfIsabella,
    BfLily,
    BmDaniel,
    BmFable,
    BmGeorge,
    BmLewis,
    EfDora,
    EmAlex,
    EmSanta,
    FfSiwis,
    HfAlpha,
    HfBeta,
    HmOmega,
    HmPsi,
    IfSara,
    ImNicola,
    JfAlpha,
    JfGongitsune,
    JfNezumi,
    JfTebukuro,
    JmKumo,
    PfDora,
    PmAlex,
    PmSanta,
    ZfXiaobei,
    ZfXiaoni,
    ZfXiaoxiao,
    ZfXiaoyi,
    ZmYunjian,
    ZmYunxi,
    ZmYunxia,
    ZmYunyang,
}

impl VoiceId {
    /// File stem of the provisioned voice bank
    /// (`voices/<stem>.bin` upstream).
    pub const fn file_stem(self) -> &'static str {
        match self {
            VoiceId::Af => "af",
            VoiceId::AfAlloy => "af_alloy",
            VoiceId::AfAoede => "af_aoede",
            VoiceId::AfBella => "af_bella",
            VoiceId::AfHeart => "af_heart",
            VoiceId::AfJessica => "af_jessica",
            VoiceId::AfKore => "af_kore",
            VoiceId::AfNicole => "af_nicole",
            VoiceId::AfNova => "af_nova",
            VoiceId::AfRiver => "af_river",
            VoiceId::AfSarah => "af_sarah",
            VoiceId::AfSky => "af_sky",
            VoiceId::AmAdam => "am_adam",
            VoiceId::AmEcho => "am_echo",
            VoiceId::AmEric => "am_eric",
            VoiceId::AmFenrir => "am_fenrir",
            VoiceId::AmLiam => "am_liam",
            VoiceId::AmMichael => "am_michael",
            VoiceId::AmOnyx => "am_onyx",
            VoiceId::AmPuck => "am_puck",
            VoiceId::AmSanta => "am_santa",
            VoiceId::BfAlice => "bf_alice",
            VoiceId::BfEmma => "bf_emma",
            VoiceId::BfIsabella => "bf_isabella",
            VoiceId::BfLily => "bf_lily",
            VoiceId::BmDaniel => "bm_daniel",
            VoiceId::BmFable => "bm_fable",
            VoiceId::BmGeorge => "bm_george",
            VoiceId::BmLewis => "bm_lewis",
            VoiceId::EfDora => "ef_dora",
            VoiceId::EmAlex => "em_alex",
            VoiceId::EmSanta => "em_santa",
            VoiceId::FfSiwis => "ff_siwis",
            VoiceId::HfAlpha => "hf_alpha",
            VoiceId::HfBeta => "hf_beta",
            VoiceId::HmOmega => "hm_omega",
            VoiceId::HmPsi => "hm_psi",
            VoiceId::IfSara => "if_sara",
            VoiceId::ImNicola => "im_nicola",
            VoiceId::JfAlpha => "jf_alpha",
            VoiceId::JfGongitsune => "jf_gongitsune",
            VoiceId::JfNezumi => "jf_nezumi",
            VoiceId::JfTebukuro => "jf_tebukuro",
            VoiceId::JmKumo => "jm_kumo",
            VoiceId::PfDora => "pf_dora",
            VoiceId::PmAlex => "pm_alex",
            VoiceId::PmSanta => "pm_santa",
            VoiceId::ZfXiaobei => "zf_xiaobei",
            VoiceId::ZfXiaoni => "zf_xiaoni",
            VoiceId::ZfXiaoxiao => "zf_xiaoxiao",
            VoiceId::ZfXiaoyi => "zf_xiaoyi",
            VoiceId::ZmYunjian => "zm_yunjian",
            VoiceId::ZmYunxi => "zm_yunxi",
            VoiceId::ZmYunxia => "zm_yunxia",
            VoiceId::ZmYunyang => "zm_yunyang",
        }
    }
}

impl VoiceId {
    /// Every provisioned voice, in provisioning order. The index
    /// doubles as the `set_speaker` id for backends that take one.
    pub const ALL: [VoiceId; 55] = [
        VoiceId::Af,
        VoiceId::AfAlloy,
        VoiceId::AfAoede,
        VoiceId::AfBella,
        VoiceId::AfHeart,
        VoiceId::AfJessica,
        VoiceId::AfKore,
        VoiceId::AfNicole,
        VoiceId::AfNova,
        VoiceId::AfRiver,
        VoiceId::AfSarah,
        VoiceId::AfSky,
        VoiceId::AmAdam,
        VoiceId::AmEcho,
        VoiceId::AmEric,
        VoiceId::AmFenrir,
        VoiceId::AmLiam,
        VoiceId::AmMichael,
        VoiceId::AmOnyx,
        VoiceId::AmPuck,
        VoiceId::AmSanta,
        VoiceId::BfAlice,
        VoiceId::BfEmma,
        VoiceId::BfIsabella,
        VoiceId::BfLily,
        VoiceId::BmDaniel,
        VoiceId::BmFable,
        VoiceId::BmGeorge,
        VoiceId::BmLewis,
        VoiceId::EfDora,
        VoiceId::EmAlex,
        VoiceId::EmSanta,
        VoiceId::FfSiwis,
        VoiceId::HfAlpha,
        VoiceId::HfBeta,
        VoiceId::HmOmega,
        VoiceId::HmPsi,
        VoiceId::IfSara,
        VoiceId::ImNicola,
        VoiceId::JfAlpha,
        VoiceId::JfGongitsune,
        VoiceId::JfNezumi,
        VoiceId::JfTebukuro,
        VoiceId::JmKumo,
        VoiceId::PfDora,
        VoiceId::PmAlex,
        VoiceId::PmSanta,
        VoiceId::ZfXiaobei,
        VoiceId::ZfXiaoni,
        VoiceId::ZfXiaoxiao,
        VoiceId::ZfXiaoyi,
        VoiceId::ZmYunjian,
        VoiceId::ZmYunxi,
        VoiceId::ZmYunxia,
        VoiceId::ZmYunyang,
    ];

    /// Voice by provisioning index, for integer speaker ids.
    pub const fn from_index(i: usize) -> Option<VoiceId> {
        if i < Self::ALL.len() {
            Some(Self::ALL[i])
        } else {
            None
        }
    }
}

/// Voice for the built-in TTS.
pub const SPEECH_VOICE: VoiceId = VoiceId::AfHeart;
