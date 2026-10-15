//! Character encoding detection, decoding, and re-encoding.
//!
//! This module is the single home for everything erax knows about text
//! encodings: BOM sniffing, deterministic encoding detection, decoding to
//! Rust `String`, byte-exact re-encoding, and the [`Buffer`](crate::core::buffer::Buffer)
//! integration that tags a loaded buffer with its detected encoding.
//!
//! # Design
//!
//! * The WHATWG Encoding Standard encodings (UTF-8, UTF-16LE/BE, the
//!   ISO-8859-* and Windows-125* families, Shift_JIS, EUC-JP/KR, Big5,
//!   GB18030, KOI8-R/U, …) are handled by `encoding_rs`, the reference
//!   implementation of that standard. Hand-rolling those decode tables would
//!   duplicate a high-impact, security-sensitive subsystem, so we don't.
//! * UTF-32LE/BE are *not* part of the WHATWG standard and are absent from
//!   `encoding_rs`. Their codec is a fixed 4-byte-unit mapping with no tables,
//!   so it is implemented directly here and heavily tested.
//! * Detection is fully deterministic: BOM sniff → NUL-byte pattern
//!   heuristic (BOM-less UTF-16/32) → UTF-8 validation → locale hint →
//!   Windows-1252 fallback. See [`detect`] for why a statistical detector
//!   (`chardetng`) was evaluated and rejected.
//!
//! # Byte-exact round trip
//!
//! For *well-formed* input (decodes without errors), [`decode`] followed by
//! [`encode`] with the same [`DetectedEncoding`] reproduces the original
//! bytes exactly, including any BOM. Malformed input never panics: it decodes
//! lossy with [`HadErrors`] set, so corruption is *reported*, never silent.
//!
//! # Binary safety
//!
//! Decoding is total over all byte strings for every encoding: single-byte
//! encodings map every byte, and malformed sequences in multi-byte encodings
//! become U+FFFD. There is no input that panics — every panic reachable from
//! input would be a bug, and the test suite bullies this property.

use std::path::{Path, PathBuf};

use crate::core::buffer::{Buffer, LoadError};

// ---------------------------------------------------------------------------
// Encoding
// ---------------------------------------------------------------------------

/// A text encoding erax can decode and encode.
///
/// This enumerates exactly the encodings supported by `encoding_rs` (the
/// WHATWG Encoding Standard), plus UTF-32LE/BE which WHATWG does not define.
///
/// Honest naming notes:
/// * There is no ISO-8859-1 variant: per WHATWG, the `iso-8859-1` / `latin1`
///   labels map to Windows-1252 (bytes 0x80–0x9F are the Windows additions).
///   Use [`Encoding::Windows1252`]; [`Encoding::from_label`] resolves the
///   aliases for you.
/// * Likewise `iso-8859-9` → [`Encoding::Windows1254`] and `iso-8859-11` →
///   [`Encoding::Windows874`] per the standard.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Encoding {
    Utf8,
    Utf16Le,
    Utf16Be,
    /// Not in WHATWG/encoding_rs: implemented directly in this module.
    Utf32Le,
    /// Not in WHATWG/encoding_rs: implemented directly in this module.
    Utf32Be,
    Ibm866,
    Iso8859_2,
    Iso8859_3,
    Iso8859_4,
    Iso8859_5,
    Iso8859_6,
    Iso8859_7,
    Iso8859_8,
    Iso8859_10,
    Iso8859_13,
    Iso8859_14,
    Iso8859_15,
    Iso8859_16,
    Koi8R,
    Koi8U,
    Macintosh,
    XMacCyrillic,
    Windows874,
    Windows1250,
    Windows1251,
    Windows1252,
    Windows1253,
    Windows1254,
    Windows1255,
    Windows1256,
    Windows1257,
    Windows1258,
    Gb18030,
    /// encoding_rs keeps WHATWG's `gbk` label as its own encoding: same
    /// decoder as GB18030, slightly different encoder (CP936 euro-sign
    /// behavior). Mapped exactly, not merged.
    Gbk,
    Big5,
    EucJp,
    Iso2022Jp,
    ShiftJis,
    EucKr,
    XUserDefined,
}

/// Every encoding, in a stable order.
pub const ALL_ENCODINGS: &[Encoding; 40] = &[
    Encoding::Utf8,
    Encoding::Utf16Le,
    Encoding::Utf16Be,
    Encoding::Utf32Le,
    Encoding::Utf32Be,
    Encoding::Ibm866,
    Encoding::Iso8859_2,
    Encoding::Iso8859_3,
    Encoding::Iso8859_4,
    Encoding::Iso8859_5,
    Encoding::Iso8859_6,
    Encoding::Iso8859_7,
    Encoding::Iso8859_8,
    Encoding::Iso8859_10,
    Encoding::Iso8859_13,
    Encoding::Iso8859_14,
    Encoding::Iso8859_15,
    Encoding::Iso8859_16,
    Encoding::Koi8R,
    Encoding::Koi8U,
    Encoding::Macintosh,
    Encoding::XMacCyrillic,
    Encoding::Windows874,
    Encoding::Windows1250,
    Encoding::Windows1251,
    Encoding::Windows1252,
    Encoding::Windows1253,
    Encoding::Windows1254,
    Encoding::Windows1255,
    Encoding::Windows1256,
    Encoding::Windows1257,
    Encoding::Windows1258,
    Encoding::Gb18030,
    Encoding::Gbk,
    Encoding::Big5,
    Encoding::EucJp,
    Encoding::Iso2022Jp,
    Encoding::ShiftJis,
    Encoding::EucKr,
    Encoding::XUserDefined,
];

impl Encoding {
    /// All encodings in a stable order.
    pub fn all() -> &'static [Encoding] {
        ALL_ENCODINGS
    }

    /// The WHATWG label for this encoding (`"utf-8"`, `"windows-1251"`, …).
    /// For UTF-32 the labels `"utf-32le"` / `"utf-32be"` are erax's own
    /// (no standard label exists).
    pub fn label(self) -> &'static str {
        match self {
            Encoding::Utf8 => "utf-8",
            Encoding::Utf16Le => "utf-16le",
            Encoding::Utf16Be => "utf-16be",
            Encoding::Utf32Le => "utf-32le",
            Encoding::Utf32Be => "utf-32be",
            Encoding::Ibm866 => "ibm866",
            Encoding::Iso8859_2 => "iso-8859-2",
            Encoding::Iso8859_3 => "iso-8859-3",
            Encoding::Iso8859_4 => "iso-8859-4",
            Encoding::Iso8859_5 => "iso-8859-5",
            Encoding::Iso8859_6 => "iso-8859-6",
            Encoding::Iso8859_7 => "iso-8859-7",
            Encoding::Iso8859_8 => "iso-8859-8",
            Encoding::Iso8859_10 => "iso-8859-10",
            Encoding::Iso8859_13 => "iso-8859-13",
            Encoding::Iso8859_14 => "iso-8859-14",
            Encoding::Iso8859_15 => "iso-8859-15",
            Encoding::Iso8859_16 => "iso-8859-16",
            Encoding::Koi8R => "koi8-r",
            Encoding::Koi8U => "koi8-u",
            Encoding::Macintosh => "macintosh",
            Encoding::XMacCyrillic => "x-mac-cyrillic",
            Encoding::Windows874 => "windows-874",
            Encoding::Windows1250 => "windows-1250",
            Encoding::Windows1251 => "windows-1251",
            Encoding::Windows1252 => "windows-1252",
            Encoding::Windows1253 => "windows-1253",
            Encoding::Windows1254 => "windows-1254",
            Encoding::Windows1255 => "windows-1255",
            Encoding::Windows1256 => "windows-1256",
            Encoding::Windows1257 => "windows-1257",
            Encoding::Windows1258 => "windows-1258",
            Encoding::Gb18030 => "gb18030",
            Encoding::Gbk => "gbk",
            Encoding::Big5 => "big5",
            Encoding::EucJp => "euc-jp",
            Encoding::Iso2022Jp => "iso-2022-jp",
            Encoding::ShiftJis => "shift_jis",
            Encoding::EucKr => "euc-kr",
            Encoding::XUserDefined => "x-user-defined",
        }
    }

    /// Resolve a WHATWG label (ASCII case-insensitive, aliases included:
    /// `"latin1"`/`"iso-8859-1"` → [`Encoding::Windows1252`],
    /// `"gbk"` → [`Encoding::Gb18030`]) or erax's `"utf-32le"`/`"utf-32be"`.
    /// Returns `None` for unknown labels.
    pub fn from_label(label: &str) -> Option<Encoding> {
        let lower = label.to_ascii_lowercase();
        if lower == "utf-32le" {
            return Some(Encoding::Utf32Le);
        }
        if lower == "utf-32be" {
            return Some(Encoding::Utf32Be);
        }
        encoding_rs::Encoding::for_label(label.as_bytes()).and_then(Encoding::from_rs)
    }

    /// True for encodings where every code unit is one byte.
    pub fn is_single_byte(self) -> bool {
        matches!(
            self,
            Encoding::Ibm866
                | Encoding::Iso8859_2
                | Encoding::Iso8859_3
                | Encoding::Iso8859_4
                | Encoding::Iso8859_5
                | Encoding::Iso8859_6
                | Encoding::Iso8859_7
                | Encoding::Iso8859_8
                | Encoding::Iso8859_10
                | Encoding::Iso8859_13
                | Encoding::Iso8859_14
                | Encoding::Iso8859_15
                | Encoding::Iso8859_16
                | Encoding::Koi8R
                | Encoding::Koi8U
                | Encoding::Macintosh
                | Encoding::XMacCyrillic
                | Encoding::Windows874
                | Encoding::Windows1250
                | Encoding::Windows1251
                | Encoding::Windows1252
                | Encoding::Windows1253
                | Encoding::Windows1254
                | Encoding::Windows1255
                | Encoding::Windows1256
                | Encoding::Windows1257
                | Encoding::Windows1258
                | Encoding::XUserDefined
        )
    }

    /// The underlying `encoding_rs` encoding, or `None` for UTF-32LE/BE
    /// (implemented directly in this module).
    fn as_rs(self) -> Option<&'static encoding_rs::Encoding> {
        match self {
            Encoding::Utf8 => Some(encoding_rs::UTF_8),
            Encoding::Utf16Le => Some(encoding_rs::UTF_16LE),
            Encoding::Utf16Be => Some(encoding_rs::UTF_16BE),
            Encoding::Utf32Le | Encoding::Utf32Be => None,
            Encoding::Ibm866 => Some(encoding_rs::IBM866),
            Encoding::Iso8859_2 => Some(encoding_rs::ISO_8859_2),
            Encoding::Iso8859_3 => Some(encoding_rs::ISO_8859_3),
            Encoding::Iso8859_4 => Some(encoding_rs::ISO_8859_4),
            Encoding::Iso8859_5 => Some(encoding_rs::ISO_8859_5),
            Encoding::Iso8859_6 => Some(encoding_rs::ISO_8859_6),
            Encoding::Iso8859_7 => Some(encoding_rs::ISO_8859_7),
            Encoding::Iso8859_8 => Some(encoding_rs::ISO_8859_8),
            Encoding::Iso8859_10 => Some(encoding_rs::ISO_8859_10),
            Encoding::Iso8859_13 => Some(encoding_rs::ISO_8859_13),
            Encoding::Iso8859_14 => Some(encoding_rs::ISO_8859_14),
            Encoding::Iso8859_15 => Some(encoding_rs::ISO_8859_15),
            Encoding::Iso8859_16 => Some(encoding_rs::ISO_8859_16),
            Encoding::Koi8R => Some(encoding_rs::KOI8_R),
            Encoding::Koi8U => Some(encoding_rs::KOI8_U),
            Encoding::Macintosh => Some(encoding_rs::MACINTOSH),
            Encoding::XMacCyrillic => Some(encoding_rs::X_MAC_CYRILLIC),
            Encoding::Windows874 => Some(encoding_rs::WINDOWS_874),
            Encoding::Windows1250 => Some(encoding_rs::WINDOWS_1250),
            Encoding::Windows1251 => Some(encoding_rs::WINDOWS_1251),
            Encoding::Windows1252 => Some(encoding_rs::WINDOWS_1252),
            Encoding::Windows1253 => Some(encoding_rs::WINDOWS_1253),
            Encoding::Windows1254 => Some(encoding_rs::WINDOWS_1254),
            Encoding::Windows1255 => Some(encoding_rs::WINDOWS_1255),
            Encoding::Windows1256 => Some(encoding_rs::WINDOWS_1256),
            Encoding::Windows1257 => Some(encoding_rs::WINDOWS_1257),
            Encoding::Windows1258 => Some(encoding_rs::WINDOWS_1258),
            Encoding::Gb18030 => Some(encoding_rs::GB18030),
            Encoding::Gbk => Some(encoding_rs::GBK),
            Encoding::Big5 => Some(encoding_rs::BIG5),
            Encoding::EucJp => Some(encoding_rs::EUC_JP),
            Encoding::Iso2022Jp => Some(encoding_rs::ISO_2022_JP),
            Encoding::ShiftJis => Some(encoding_rs::SHIFT_JIS),
            Encoding::EucKr => Some(encoding_rs::EUC_KR),
            Encoding::XUserDefined => Some(encoding_rs::X_USER_DEFINED),
        }
    }

    /// Map an `encoding_rs` encoding back to [`Encoding`].
    /// Returns `None` for encodings with no erax counterpart (e.g.
    /// `REPLACEMENT`, which `for_label("replacement")` can produce).
    fn from_rs(enc: &'static encoding_rs::Encoding) -> Option<Encoding> {
        // `encoding_rs::Encoding` equality is pointer equality over its
        // static table, so this chain is a cheap identity check per entry.
        if enc == encoding_rs::UTF_8 {
            Some(Encoding::Utf8)
        } else if enc == encoding_rs::UTF_16LE {
            Some(Encoding::Utf16Le)
        } else if enc == encoding_rs::UTF_16BE {
            Some(Encoding::Utf16Be)
        } else if enc == encoding_rs::IBM866 {
            Some(Encoding::Ibm866)
        } else if enc == encoding_rs::ISO_8859_2 {
            Some(Encoding::Iso8859_2)
        } else if enc == encoding_rs::ISO_8859_3 {
            Some(Encoding::Iso8859_3)
        } else if enc == encoding_rs::ISO_8859_4 {
            Some(Encoding::Iso8859_4)
        } else if enc == encoding_rs::ISO_8859_5 {
            Some(Encoding::Iso8859_5)
        } else if enc == encoding_rs::ISO_8859_6 {
            Some(Encoding::Iso8859_6)
        } else if enc == encoding_rs::ISO_8859_7 {
            Some(Encoding::Iso8859_7)
        } else if enc == encoding_rs::ISO_8859_8 {
            Some(Encoding::Iso8859_8)
        } else if enc == encoding_rs::ISO_8859_10 {
            Some(Encoding::Iso8859_10)
        } else if enc == encoding_rs::ISO_8859_13 {
            Some(Encoding::Iso8859_13)
        } else if enc == encoding_rs::ISO_8859_14 {
            Some(Encoding::Iso8859_14)
        } else if enc == encoding_rs::ISO_8859_15 {
            Some(Encoding::Iso8859_15)
        } else if enc == encoding_rs::ISO_8859_16 {
            Some(Encoding::Iso8859_16)
        } else if enc == encoding_rs::KOI8_R {
            Some(Encoding::Koi8R)
        } else if enc == encoding_rs::KOI8_U {
            Some(Encoding::Koi8U)
        } else if enc == encoding_rs::MACINTOSH {
            Some(Encoding::Macintosh)
        } else if enc == encoding_rs::X_MAC_CYRILLIC {
            Some(Encoding::XMacCyrillic)
        } else if enc == encoding_rs::WINDOWS_874 {
            Some(Encoding::Windows874)
        } else if enc == encoding_rs::WINDOWS_1250 {
            Some(Encoding::Windows1250)
        } else if enc == encoding_rs::WINDOWS_1251 {
            Some(Encoding::Windows1251)
        } else if enc == encoding_rs::WINDOWS_1252 {
            Some(Encoding::Windows1252)
        } else if enc == encoding_rs::WINDOWS_1253 {
            Some(Encoding::Windows1253)
        } else if enc == encoding_rs::WINDOWS_1254 {
            Some(Encoding::Windows1254)
        } else if enc == encoding_rs::WINDOWS_1255 {
            Some(Encoding::Windows1255)
        } else if enc == encoding_rs::WINDOWS_1256 {
            Some(Encoding::Windows1256)
        } else if enc == encoding_rs::WINDOWS_1257 {
            Some(Encoding::Windows1257)
        } else if enc == encoding_rs::WINDOWS_1258 {
            Some(Encoding::Windows1258)
        } else if enc == encoding_rs::GB18030 {
            Some(Encoding::Gb18030)
        } else if enc == encoding_rs::GBK {
            Some(Encoding::Gbk)
        } else if enc == encoding_rs::BIG5 {
            Some(Encoding::Big5)
        } else if enc == encoding_rs::EUC_JP {
            Some(Encoding::EucJp)
        } else if enc == encoding_rs::ISO_2022_JP {
            Some(Encoding::Iso2022Jp)
        } else if enc == encoding_rs::SHIFT_JIS {
            Some(Encoding::ShiftJis)
        } else if enc == encoding_rs::EUC_KR {
            Some(Encoding::EucKr)
        } else if enc == encoding_rs::X_USER_DEFINED {
            Some(Encoding::XUserDefined)
        } else {
            None
        }
    }
}

// ---------------------------------------------------------------------------
// BOM
// ---------------------------------------------------------------------------

/// A byte-order mark found at the start of the input.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Bom {
    Utf8,
    Utf16Le,
    Utf16Be,
    Utf32Le,
    Utf32Be,
}

impl Bom {
    /// The raw BOM bytes.
    pub fn bytes(self) -> &'static [u8] {
        match self {
            Bom::Utf8 => b"\xEF\xBB\xBF",
            Bom::Utf16Le => b"\xFF\xFE",
            Bom::Utf16Be => b"\xFE\xFF",
            Bom::Utf32Le => b"\xFF\xFE\x00\x00",
            Bom::Utf32Be => b"\x00\x00\xFE\xFF",
        }
    }

    /// The encoding a BOM implies.
    pub fn encoding(self) -> Encoding {
        match self {
            Bom::Utf8 => Encoding::Utf8,
            Bom::Utf16Le => Encoding::Utf16Le,
            Bom::Utf16Be => Encoding::Utf16Be,
            Bom::Utf32Le => Encoding::Utf32Le,
            Bom::Utf32Be => Encoding::Utf32Be,
        }
    }

    /// The BOM that implies `encoding`, if it is a BOM-carrying encoding.
    pub fn for_encoding(encoding: Encoding) -> Option<Bom> {
        match encoding {
            Encoding::Utf8 => Some(Bom::Utf8),
            Encoding::Utf16Le => Some(Bom::Utf16Le),
            Encoding::Utf16Be => Some(Bom::Utf16Be),
            Encoding::Utf32Le => Some(Bom::Utf32Le),
            Encoding::Utf32Be => Some(Bom::Utf32Be),
            _ => None,
        }
    }
}

// ---------------------------------------------------------------------------
// Detection
// ---------------------------------------------------------------------------

/// How the encoding of an input was determined.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DetectionMethod {
    /// A byte-order mark was present (highest authority).
    Bom,
    /// The bytes are valid UTF-8 (byte-exact, no loss possible).
    Utf8Validation,
    /// NUL-byte position pattern matched BOM-less UTF-16/32.
    NulPattern,
    /// The user's locale suggested a legacy encoding (fallback only).
    LocaleHint,
    /// Explicit user override — always honored.
    Explicit,
    /// Deterministic last resort: Windows-1252 (see [`detect`]).
    Fallback,
}

/// How much the detection result should be trusted.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Confidence {
    /// Genuinely ambiguous (e.g. BOM-less single-byte text): the result is a
    /// safe total decoding, but the user should be able to override it.
    Low,
    /// Strong structural evidence, but not proof (NUL-pattern heuristic).
    Medium,
    /// Proof or explicit user choice (BOM, UTF-8 validation, override).
    High,
}

/// The outcome of [`detect`]: which encoding, whether a BOM was present
/// (needed for byte-exact re-encoding), how it was determined, and how much
/// to trust it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DetectedEncoding {
    pub encoding: Encoding,
    pub bom: Option<Bom>,
    pub method: DetectionMethod,
    pub confidence: Confidence,
}

impl DetectedEncoding {
    /// Plain UTF-8, no BOM, validated — the common case.
    pub fn utf8() -> Self {
        Self {
            encoding: Encoding::Utf8,
            bom: None,
            method: DetectionMethod::Utf8Validation,
            confidence: Confidence::High,
        }
    }

    /// Explicit user override: always honored, reported as such.
    pub fn explicit(encoding: Encoding) -> Self {
        Self {
            encoding,
            bom: None,
            method: DetectionMethod::Explicit,
            confidence: Confidence::High,
        }
    }
}

/// Detect the encoding of `bytes`.
///
/// The pipeline is fully deterministic — the same bytes always give the same
/// answer, and every answer carries its [`DetectionMethod`] and
/// [`Confidence`] so callers (and the UI) can decide how much to trust it:
///
/// 1. **BOM sniff** — UTF-8/16/32 LE/BE, longest match first (the UTF-32LE
///    BOM `FF FE 00 00` starts with the UTF-16LE BOM bytes). High confidence.
/// 2. **NUL-byte pattern heuristic** — BOM-less UTF-16/32 text in the ASCII
///    range has NULs in highly regular positions (every odd byte for
///    UTF-16LE, 3-of-4 for UTF-32LE, …). Medium confidence. Known limitation:
///    UTF-16/32 text outside the ASCII range (e.g. CJK) has no NUL pattern
///    and falls through — the result stays honest (`Low` fallback) instead of
///    guessing.
/// 3. **UTF-8 validation** — valid UTF-8 is unambiguous and byte-exact.
///    High confidence.
/// 4. **Windows-1252 fallback** — Low confidence, but never wrong *as a
///    decoding*: Windows-1252 (the WHATWG merger of ISO-8859-1) is a total
///    function over bytes, so decoding never fails, never panics, and never
///    loses a byte. It is reported as a guess, not a fact.
///
/// Tie-break: an input that is *both* valid UTF-8 *and* strongly
/// NUL-patterned (e.g. ASCII text as BOM-less UTF-16LE, whose bytes are also
/// legal UTF-8 with embedded NULs) is genuinely ambiguous — no detector can
/// resolve it. It is read as UTF-16/32: NULs at every other byte position
/// almost always mean UTF-16 (NULs are vanishingly rare in real UTF-8 text),
/// and the decode→encode round trip stays byte-exact either way, so nothing
/// is lost and the explicit override remains available.
///
/// # Why not `chardetng`?
///
/// A statistical detector was evaluated and deliberately rejected:
/// * It would be a *second* new dependency (this module is allowed exactly
///   one: `encoding_rs`), pulling in large statistical tables for a guess
///   that is still wrong on short inputs and binary data — exactly the
///   adversarial inputs this module must handle.
/// * Mozilla's own guidance is that universal detection needs long text runs;
///   an editor routinely opens short files, source code, and raw binary.
/// * The honest engineering trade is: deterministic evidence where it exists
///   (BOM, UTF-8 validity, NUL structure), a total safe fallback where it
///   doesn't, and an explicit user override ([`DetectedEncoding::explicit`]
///   / [`Encoding::from_label`]) for the genuinely ambiguous cases. A
///   statistical guess with false confidence is worse than an honest
///   low-confidence fallback.
pub fn detect(bytes: &[u8]) -> DetectedEncoding {
    detect_impl(bytes, None)
}

/// Like [`detect`], but if the deterministic pipeline reaches the fallback,
/// the locale hint's preferred encoding is used instead (still reported with
/// [`DetectionMethod::LocaleHint`] and [`Confidence::Low`]).
///
/// The hint never overrides real evidence: BOM, UTF-8 validity, and the NUL
/// pattern always win. Use [`locale_hint_from_env`] to build the hint from
/// `LC_ALL` / `LC_CTYPE` / `LANG`.
pub fn detect_with_hint(bytes: &[u8], hint: &LocaleHint) -> DetectedEncoding {
    detect_impl(bytes, Some(hint))
}

fn detect_impl(bytes: &[u8], hint: Option<&LocaleHint>) -> DetectedEncoding {
    // 1. BOM sniff — 4-byte BOMs first: FF FE 00 00 also starts with FF FE.
    if bytes.starts_with(Bom::Utf32Le.bytes()) {
        return DetectedEncoding {
            encoding: Encoding::Utf32Le,
            bom: Some(Bom::Utf32Le),
            method: DetectionMethod::Bom,
            confidence: Confidence::High,
        };
    }
    if bytes.starts_with(Bom::Utf32Be.bytes()) {
        return DetectedEncoding {
            encoding: Encoding::Utf32Be,
            bom: Some(Bom::Utf32Be),
            method: DetectionMethod::Bom,
            confidence: Confidence::High,
        };
    }
    if let Some((rs_enc, _bom_len)) = encoding_rs::Encoding::for_bom(bytes)
        && let Some(encoding) = Encoding::from_rs(rs_enc)
        && let Some(bom) = Bom::for_encoding(encoding)
    {
        return DetectedEncoding {
            encoding,
            bom: Some(bom),
            method: DetectionMethod::Bom,
            confidence: Confidence::High,
        };
    }

    // 2. NUL-byte pattern heuristic for BOM-less UTF-16/32. Runs before
    //    UTF-8 validation on purpose: BOM-less UTF-16LE ASCII text is also
    //    valid UTF-8 (with embedded NULs), so validating first would make
    //    this heuristic dead for the most common UTF-16 content. See the
    //    tie-break note on `detect`.
    if let Some(encoding) = detect_by_nul_pattern(bytes) {
        return DetectedEncoding {
            encoding,
            bom: None,
            method: DetectionMethod::NulPattern,
            confidence: Confidence::Medium,
        };
    }

    // 3. UTF-8 validation: unambiguous and byte-exact.
    if std::str::from_utf8(bytes).is_ok() {
        return DetectedEncoding::utf8();
    }

    // 4. Locale hint, then the deterministic fallback.
    if let Some(h) = hint
        && let Some(preferred) = h.preferred
    {
        return DetectedEncoding {
            encoding: preferred,
            bom: None,
            method: DetectionMethod::LocaleHint,
            confidence: Confidence::Low,
        };
    }
    DetectedEncoding {
        encoding: Encoding::Windows1252,
        bom: None,
        method: DetectionMethod::Fallback,
        confidence: Confidence::Low,
    }
}

/// Bytes of input examined by the NUL-pattern heuristic. Detection stays
/// O(1) in file size; only decoding touches the whole input.
const NUL_SAMPLE_LEN: usize = 32 * 1024;

/// Minimum NUL fraction for the input to plausibly be UTF-16/32 text.
/// ASCII-range UTF-16 text is ~50% NUL, UTF-32 ~75%; random binary with
/// scattered NULs scores far lower on the *positional* test anyway, but the
/// floor keeps tiny/degenerate inputs out.
const NUL_FRACTION_FLOOR: f64 = 0.15;

/// Minimum positional score for the UTF-32 patterns. Deliberately strict:
/// UTF-16 text scores ~0.67 on the UTF-32 pattern (2 of every 3 pattern
/// positions), and UTF-16 text with emoji/non-BMP units can score ~0.79, so
/// the bar must clear both with margin.
const NUL_STRICT_THRESHOLD: f64 = 0.85;

/// Minimum NUL majority for the UTF-16 patterns. Lower than the UTF-32 bar
/// on purpose: mixed ASCII/non-ASCII UTF-16 text (e.g. `"Hello, 世界"`) has
/// NULs at only some of the pattern positions, but the *asymmetry* (NULs
/// strongly favoring one parity while the other stays nearly NUL-free) is
/// still decisive — binary data never looks like that.
const NUL_MAJORITY_THRESHOLD: f64 = 0.5;

/// Maximum NUL fraction allowed at the *off-pattern* positions. Real
/// UTF-16/32 text has essentially no NULs there (only literal U+0000 units,
/// which are pathological in text).
const NUL_MINORITY_THRESHOLD: f64 = 0.2;

/// Heuristic detection of BOM-less UTF-16LE/BE and UTF-32LE/BE via NUL-byte
/// positions. Returns `None` when the pattern is absent or ambiguous.
///
/// Positional signatures for ASCII-range text:
/// * UTF-16LE: NUL at every odd byte (`H\0e\0l\0l\0o\0`).
/// * UTF-16BE: NUL at every even byte (`\0H\0e\0l\0l\0o`).
/// * UTF-32LE: NUL at every byte with `i % 4 != 0`.
/// * UTF-32BE: NUL at every byte with `i % 4 != 3`.
///
/// UTF-32 is checked first with the strict bar: its signature also matches
/// the UTF-16 positional test (odd bytes are all NUL in UTF-32LE), so the
/// narrower pattern must win ties.
fn detect_by_nul_pattern(bytes: &[u8]) -> Option<Encoding> {
    let sample_len = bytes.len().min(NUL_SAMPLE_LEN);
    if sample_len < 16 {
        return None;
    }
    let sample = &bytes[..sample_len];

    let mut nul_total: u32 = 0;
    let mut even_nul: u32 = 0;
    let mut even_total: u32 = 0;
    let mut odd_nul: u32 = 0;
    let mut odd_total: u32 = 0;
    let mut le_nul: u32 = 0; // UTF-32LE: NUL expected at i%4 != 0
    let mut le_total: u32 = 0;
    let mut be_nul: u32 = 0; // UTF-32BE: NUL expected at i%4 != 3
    let mut be_total: u32 = 0;
    let mut mod0_nul: u32 = 0; // UTF-32LE: NUL *not* expected at i%4 == 0
    let mut mod0_total: u32 = 0;
    let mut mod3_nul: u32 = 0; // UTF-32BE: NUL *not* expected at i%4 == 3
    let mut mod3_total: u32 = 0;

    for (i, &b) in sample.iter().enumerate() {
        let is_nul = u32::from(b == 0);
        nul_total += is_nul;
        if i % 2 == 0 {
            even_total += 1;
            even_nul += is_nul;
        } else {
            odd_total += 1;
            odd_nul += is_nul;
        }
        match i % 4 {
            0 => {
                mod0_total += 1;
                mod0_nul += is_nul;
                be_total += 1;
                be_nul += is_nul;
            }
            3 => {
                mod3_total += 1;
                mod3_nul += is_nul;
                le_total += 1;
                le_nul += is_nul;
            }
            _ => {
                le_total += 1;
                le_nul += is_nul;
                be_total += 1;
                be_nul += is_nul;
            }
        }
    }

    let nul_fraction = f64::from(nul_total) / sample_len as f64;
    if nul_fraction < NUL_FRACTION_FLOOR {
        return None;
    }
    let frac = |num: u32, den: u32| f64::from(num) / f64::from(den);
    let even = frac(even_nul, even_total);
    let odd = frac(odd_nul, odd_total);
    let le = frac(le_nul, le_total);
    let be = frac(be_nul, be_total);
    let mod0 = frac(mod0_nul, mod0_total);
    let mod3 = frac(mod3_nul, mod3_total);

    // UTF-32 first, strict bar (see NUL_STRICT_THRESHOLD rationale).
    if le >= NUL_STRICT_THRESHOLD && mod0 <= NUL_MINORITY_THRESHOLD {
        return Some(Encoding::Utf32Le);
    }
    if be >= NUL_STRICT_THRESHOLD && mod3 <= NUL_MINORITY_THRESHOLD {
        return Some(Encoding::Utf32Be);
    }
    // UTF-16: majority on one parity, near-silence on the other. Catches
    // mixed ASCII/non-ASCII text that never reaches the strict bar.
    if odd >= NUL_MAJORITY_THRESHOLD && even <= NUL_MINORITY_THRESHOLD {
        return Some(Encoding::Utf16Le);
    }
    if even >= NUL_MAJORITY_THRESHOLD && odd <= NUL_MINORITY_THRESHOLD {
        return Some(Encoding::Utf16Be);
    }
    None
}

// ---------------------------------------------------------------------------
// Locale awareness
// ---------------------------------------------------------------------------

/// A locale-derived hint for the genuinely ambiguous case (BOM-less,
/// non-UTF-8, non-UTF-16/32 bytes — i.e. legacy single-byte text where every
/// byte is valid and statistical detection was rejected).
///
/// The hint only ever replaces the [`DetectionMethod::Fallback`]; it never
/// overrides BOM, UTF-8, or NUL-pattern evidence.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct LocaleHint {
    /// Preferred legacy encoding suggested by the locale, if the locale maps
    /// to a known one.
    pub preferred: Option<Encoding>,
    /// Lowercase language subtag parsed from the locale (`"ru"`, `"ja"`, …),
    /// informational.
    pub language: Option<String>,
}

/// Build a [`LocaleHint`] from `LC_ALL`, `LC_CTYPE`, `LANG` (first non-empty
/// wins). Returns a hint with `preferred: None` when the locale is missing,
/// unparseable, or maps to no known legacy encoding (e.g. `C.UTF-8`).
pub fn locale_hint_from_env() -> LocaleHint {
    let raw = std::env::var("LC_ALL")
        .or_else(|_| std::env::var("LC_CTYPE"))
        .or_else(|_| std::env::var("LANG"))
        .unwrap_or_default();
    locale_hint_from_str(&raw)
}

/// Parse a locale string like `"ru_RU.UTF-8"` or `"ja_JP.eucJP"` into a hint.
/// Split out for testability; [`locale_hint_from_env`] is the real entry point.
fn locale_hint_from_str(locale: &str) -> LocaleHint {
    // Language subtag: before '.', '@', or '_' — e.g. "ru_RU.UTF-8" -> "ru".
    let lang_part = locale.split(['.', '@']).next().unwrap_or("");
    let language = lang_part
        .split('_')
        .next()
        .unwrap_or("")
        .to_ascii_lowercase();
    let preferred = match language.as_str() {
        "ru" | "uk" | "be" | "bg" | "sr" | "mk" => Some(Encoding::Windows1251),
        "tr" => Some(Encoding::Windows1254),
        "el" => Some(Encoding::Windows1253),
        "he" => Some(Encoding::Windows1255),
        "ar" | "fa" | "ur" => Some(Encoding::Windows1256),
        "pl" | "cs" | "hu" | "ro" | "hr" | "sk" | "sl" => Some(Encoding::Windows1250),
        "lt" | "lv" | "et" => Some(Encoding::Windows1257),
        "vi" => Some(Encoding::Windows1258),
        "th" => Some(Encoding::Windows874),
        "ja" => Some(Encoding::ShiftJis),
        "ko" => Some(Encoding::EucKr),
        "zh" => {
            if lang_part.starts_with("zh_TW") || lang_part.starts_with("zh_HK") {
                Some(Encoding::Big5)
            } else {
                Some(Encoding::Gb18030)
            }
        }
        _ => None,
    };
    LocaleHint {
        preferred,
        language: if language.is_empty() {
            None
        } else {
            Some(language)
        },
    }
}

// ---------------------------------------------------------------------------
// Decode / encode
// ---------------------------------------------------------------------------

/// Whether decoding (or encoding) had to replace malformed/unmappable input.
/// Replacements are *reported* here, never silent.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HadErrors(pub bool);

impl HadErrors {
    /// True if any replacement happened.
    pub fn has_errors(self) -> bool {
        self.0
    }
}

/// Decode `bytes` with `encoding` into a Rust `String`.
///
/// * Total over all inputs: malformed sequences become U+FFFD and set
///   [`HadErrors`]; nothing panics, nothing is silently dropped.
/// * Mirrors `TextDecoder` BOM semantics: one leading BOM appropriate to the
///   encoding is stripped (and not part of the returned text); use
///   [`detect`] to learn whether a BOM was present.
/// * Well-formed input round-trips byte-exact through [`encode`] with the
///   matching [`DetectedEncoding`] (BOM re-added there).
pub fn decode(bytes: &[u8], encoding: Encoding) -> (String, HadErrors) {
    match encoding {
        Encoding::Utf32Le | Encoding::Utf32Be => {
            let little_endian = encoding == Encoding::Utf32Le;
            let bom = if little_endian {
                Bom::Utf32Le
            } else {
                Bom::Utf32Be
            };
            let body = if bytes.starts_with(bom.bytes()) {
                &bytes[bom.bytes().len()..]
            } else {
                bytes
            };
            let (text, had_errors) = decode_utf32(body, little_endian);
            (text, HadErrors(had_errors))
        }
        _ => {
            // `as_rs` is `Some` for every non-UTF-32 encoding by construction.
            if let Some(rs) = encoding.as_rs() {
                let (text, had_errors) = rs.decode_with_bom_removal(bytes);
                (text.into_owned(), HadErrors(had_errors))
            } else {
                // Unreachable: UTF-32 arms return above.
                (String::new(), HadErrors(true))
            }
        }
    }
}

/// Decode `bytes` with [`detect`]ed encoding. Convenience for the common
/// load path; [`load_file_with_encoding`] also tags the [`Buffer`].
pub fn decode_detected(bytes: &[u8]) -> (String, DetectedEncoding, HadErrors) {
    let detected = detect(bytes);
    let (text, had_errors) = decode(bytes, detected.encoding);
    (text, detected, had_errors)
}

/// Decode UTF-32: fixed 4-byte units, validated as Unicode scalar values.
/// Lone surrogates, out-of-range values, and a truncated tail become U+FFFD
/// with `had_errors = true`. Never panics.
fn decode_utf32(bytes: &[u8], little_endian: bool) -> (String, bool) {
    let mut out = String::new();
    let mut had_errors = false;
    let (chunks, remainder) = bytes.as_chunks::<4>();
    for c in chunks {
        let v = if little_endian {
            u32::from_le_bytes(*c)
        } else {
            u32::from_be_bytes(*c)
        };
        match char::from_u32(v) {
            Some(ch) => out.push(ch),
            None => {
                out.push('\u{FFFD}');
                had_errors = true;
            }
        }
    }
    if !remainder.is_empty() {
        // Truncated tail: 1-3 leftover bytes are not a valid unit.
        out.push('\u{FFFD}');
        had_errors = true;
    }
    (out, had_errors)
}

/// Encode UTF-16 from `char::encode_utf16` units.
///
/// Done manually (not via `encoding_rs`) for a subtle spec reason: WHATWG's
/// *encode* algorithm maps UTF-16BE/LE to UTF-8 output
/// (`output_encoding()`), so `encoding_rs::UTF_16LE.encode("A")` yields
/// `[0x41]`, not `[0x41, 0x00]`. Verified against encoding_rs 0.8.42 source.
/// Every `&str` is valid here, so this is infallible and exact.
fn encode_utf16(text: &str, little_endian: bool, out: &mut Vec<u8>) {
    // UTF-16 units <= UTF-8 bytes, so `len * 2` cannot overflow (len <= isize::MAX).
    out.reserve(text.len() * 2);
    for u in text.encode_utf16() {
        let bytes = if little_endian {
            u.to_le_bytes()
        } else {
            u.to_be_bytes()
        };
        out.extend_from_slice(&bytes);
    }
}

/// Encode UTF-32: every `char` is a valid scalar value by construction, so
/// this is infallible and exact.
fn encode_utf32(text: &str, little_endian: bool, out: &mut Vec<u8>) {
    // Each char yields exactly 4 bytes; `text.len()` (bytes) >= char count,
    // so this reserves enough without ever over-allocating pathologically.
    out.reserve(text.len());
    for ch in text.chars() {
        let v = ch as u32;
        let bytes = if little_endian {
            v.to_le_bytes()
        } else {
            v.to_be_bytes()
        };
        out.extend_from_slice(&bytes);
    }
}

/// Encode `text` back to bytes with the detected encoding, re-adding the BOM
/// recorded in `detected`.
///
/// Together with [`decode`], this gives the byte-exact round-trip guarantee:
/// for well-formed input, `encode(&decode(bytes).0, &detect(bytes)) == bytes`.
///
/// Characters unmappable to the target encoding are replaced per the WHATWG
/// standard (HTML numeric character references, e.g. `&#128512;`) — use
/// [`encode_checked`] to learn whether that happened. This is *encoding*, not
/// *decoding*: it can only trigger on text the user typed or that came from a
/// different encoding, never on a clean decode→encode round trip.
pub fn encode(text: &str, detected: &DetectedEncoding) -> Vec<u8> {
    encode_checked(text, detected).0
}

/// Like [`encode`], but also reports unmappable characters.
pub fn encode_checked(text: &str, detected: &DetectedEncoding) -> (Vec<u8>, HadErrors) {
    let mut out = Vec::new();
    if let Some(bom) = detected.bom {
        out.extend_from_slice(bom.bytes());
    }
    let had_errors = match detected.encoding {
        Encoding::Utf16Le => {
            encode_utf16(text, true, &mut out);
            false
        }
        Encoding::Utf16Be => {
            encode_utf16(text, false, &mut out);
            false
        }
        Encoding::Utf32Le => {
            encode_utf32(text, true, &mut out);
            false
        }
        Encoding::Utf32Be => {
            encode_utf32(text, false, &mut out);
            false
        }
        _ => {
            if let Some(rs) = detected.encoding.as_rs() {
                let (bytes, _, had_errors) = rs.encode(text);
                out.extend_from_slice(&bytes);
                had_errors
            } else {
                // Unreachable: UTF-32 arms return above.
                true
            }
        }
    };
    (out, HadErrors(had_errors))
}

// ---------------------------------------------------------------------------
// Buffer integration
// ---------------------------------------------------------------------------

/// A [`Buffer`] loaded with encoding detection, plus the detection report.
///
/// This is the minimal honest integration that does not change `buffer.rs`:
/// it reuses the typed [`LoadError`] path (`InvalidUtf8` already carries the
/// raw bytes) and builds the [`Buffer`] purely through public API.
#[derive(Debug)]
pub struct LoadedBuffer {
    /// The buffer. For non-UTF-8 / erroring input, `original_bytes` is set so
    /// an *unmodified* buffer saves byte-exact (existing N-143 semantics).
    pub buffer: Buffer,
    /// What was detected, including any BOM (needed for byte-exact encode).
    pub detected: DetectedEncoding,
    /// True if decoding replaced malformed sequences (reported, not silent).
    pub had_errors: bool,
    /// True when `buffer.original_bytes` preserves the source bytes.
    pub byte_exact: bool,
}

/// Build a [`LoadedBuffer`] from raw bytes (no IO performed).
///
/// Byte-exactness contract: when the input is plain BOM-less UTF-8 that
/// decodes cleanly, the rope *is* the byte-exact content and no preservation
/// copy is kept. Otherwise the original bytes are preserved in
/// `buffer.original_bytes`, so saving an unmodified buffer writes them back
/// verbatim (N-143 semantics extended to every encoding and to raw binary).
pub fn buffer_from_bytes(path: PathBuf, bytes: Vec<u8>) -> LoadedBuffer {
    let detected = detect(&bytes);
    let (text, HadErrors(had_errors)) = decode(&bytes, detected.encoding);
    let mut buffer = Buffer::from_string(text);
    buffer.filename = Some(path);
    let byte_exact = detected.encoding != Encoding::Utf8 || had_errors;
    if byte_exact {
        buffer.original_bytes = Some(bytes);
    }
    LoadedBuffer {
        buffer,
        detected,
        had_errors,
        byte_exact,
    }
}

/// Load a file like [`Buffer::from_file`], but instead of failing on
/// non-UTF-8 bytes, detect the encoding and return a buffer tagged with it.
///
/// * Valid UTF-8 (the common case) behaves exactly like `from_file`, plus a
///   UTF-8 BOM is reported when present.
/// * Anything else goes through [`buffer_from_bytes`]: the buffer holds the
///   decoded text for display/editing, keeps the original bytes for
///   byte-exact save while unmodified, and reports the detection
///   (method + confidence) so the UI can show it and offer an explicit
///   encoding override.
///
/// `LoadError::Io` and `LoadError::InvalidPath` pass through unchanged.
pub fn load_file_with_encoding(path: impl AsRef<Path>) -> Result<LoadedBuffer, LoadError> {
    match Buffer::from_file(path.as_ref()) {
        Ok(buffer) => {
            // Valid UTF-8 by construction. `char_at` is boundary-safe, so
            // this BOM check cannot panic (unlike byte-slicing the rope).
            let bom = if buffer.char_at(0) == Some('\u{FEFF}') {
                Some(Bom::Utf8)
            } else {
                None
            };
            Ok(LoadedBuffer {
                buffer,
                detected: DetectedEncoding {
                    encoding: Encoding::Utf8,
                    bom,
                    method: DetectionMethod::Utf8Validation,
                    confidence: Confidence::High,
                },
                had_errors: false,
                byte_exact: false,
            })
        }
        Err(LoadError::InvalidUtf8 { path, bytes, .. }) => Ok(buffer_from_bytes(path, bytes)),
        Err(other) => Err(other),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn explicit(encoding: Encoding) -> DetectedEncoding {
        DetectedEncoding::explicit(encoding)
    }

    fn utf16le_bytes(text: &str) -> Vec<u8> {
        let mut v = Vec::new();
        for u in text.encode_utf16() {
            v.extend_from_slice(&u.to_le_bytes());
        }
        v
    }

    fn utf16be_bytes(text: &str) -> Vec<u8> {
        let mut v = Vec::new();
        for u in text.encode_utf16() {
            v.extend_from_slice(&u.to_be_bytes());
        }
        v
    }

    fn utf32le_bytes(text: &str) -> Vec<u8> {
        let mut v = Vec::new();
        for ch in text.chars() {
            v.extend_from_slice(&(ch as u32).to_le_bytes());
        }
        v
    }

    fn utf32be_bytes(text: &str) -> Vec<u8> {
        let mut v = Vec::new();
        for ch in text.chars() {
            v.extend_from_slice(&(ch as u32).to_be_bytes());
        }
        v
    }

    // ---------------- BOM detection ----------------

    #[test]
    fn bom_detection_all_five() {
        let cases: &[(&[u8], Encoding, Bom)] = &[
            (b"\xEF\xBB\xBFhi", Encoding::Utf8, Bom::Utf8),
            (b"\xFF\xFEh\x00", Encoding::Utf16Le, Bom::Utf16Le),
            (b"\xFE\xFF\x00h", Encoding::Utf16Be, Bom::Utf16Be),
            (
                b"\xFF\xFE\x00\x00h\x00\x00\x00",
                Encoding::Utf32Le,
                Bom::Utf32Le,
            ),
            (
                b"\x00\x00\xFE\xFF\x00\x00\x00h",
                Encoding::Utf32Be,
                Bom::Utf32Be,
            ),
        ];
        for (bytes, encoding, bom) in cases {
            let d = detect(bytes);
            assert_eq!(d.encoding, *encoding, "bytes: {bytes:02x?}");
            assert_eq!(d.bom, Some(*bom));
            assert_eq!(d.method, DetectionMethod::Bom);
            assert_eq!(d.confidence, Confidence::High);
        }
    }

    #[test]
    fn utf32le_bom_beats_utf16le_prefix() {
        // FF FE 00 00 starts with the UTF-16LE BOM bytes; longest match wins.
        let d = detect(b"\xFF\xFE\x00\x00");
        assert_eq!(d.encoding, Encoding::Utf32Le);
        assert_eq!(d.bom, Some(Bom::Utf32Le));
    }

    #[test]
    fn bom_only_files_decode_to_empty_and_round_trip() {
        for bom in [Bom::Utf8, Bom::Utf16Le, Bom::Utf16Be, Bom::Utf32Le, Bom::Utf32Be] {
            let bytes = bom.bytes();
            let (text, detected, had_errors) = decode_detected(bytes);
            assert_eq!(text, "", "BOM-only file should decode to empty");
            assert!(!had_errors.has_errors());
            assert_eq!(detected.bom, Some(bom));
            // Byte-exact round trip: decode -> encode == original bytes.
            assert_eq!(encode(&text, &detected), bytes);
        }
    }

    #[test]
    fn mixed_boms_bom_wins_honestly() {
        // UTF-8 BOM followed by UTF-16LE content: genuinely ambiguous input.
        // The BOM is real evidence and wins; decoding is lossy-but-safe.
        let mut bytes = b"\xEF\xBB\xBF".to_vec();
        bytes.extend_from_slice(b"h\x00i\x00");
        let d = detect(&bytes);
        assert_eq!(d.encoding, Encoding::Utf8);
        assert_eq!(d.method, DetectionMethod::Bom);
        let (text, _) = decode(&bytes, d.encoding);
        assert!(text.contains('h'), "must not panic or lose the ASCII");
    }

    // ---------------- UTF-8 / fallback ----------------

    #[test]
    fn plain_utf8_detection() {
        for text in ["", "hello", "café €", "日本語 🎉", "a\0b\0c", "\u{FEFF}bom-then-text"] {
            let d = detect(text.as_bytes());
            assert_eq!(d.encoding, Encoding::Utf8, "text: {text:?}");
            assert_eq!(d.confidence, Confidence::High);
        }
        // Scattered (non-positional) NULs in longer input stay UTF-8: the
        // NUL-pattern heuristic requires >=85% positional regularity.
        let mut scattered = b"the quick brown fox jumps over the lazy dog 1234567890".to_vec();
        for &pos in &[3usize, 11, 23, 37, 52] {
            scattered[pos] = 0;
        }
        let d = detect(&scattered);
        assert_eq!(d.encoding, Encoding::Utf8, "scattered NULs are not UTF-16");
    }

    #[test]
    fn invalid_utf8_falls_back_to_windows1252_low() {
        // (Deliberately not starting with FF FE: that is the UTF-16LE BOM.)
        let bytes = b"\x80\x81\xFDhello\xFF\xFE";
        let d = detect(bytes);
        assert_eq!(d.encoding, Encoding::Windows1252);
        assert_eq!(d.method, DetectionMethod::Fallback);
        assert_eq!(d.confidence, Confidence::Low);
        // The fallback is total: every byte decodes, nothing panics.
        let (text, had) = decode(bytes, d.encoding);
        assert!(!had.has_errors(), "single-byte decode is total");
        assert_eq!(text.chars().count(), 10);
    }

    // ---------------- NUL-pattern heuristic ----------------

    #[test]
    fn nul_pattern_detects_bomless_utf16() {
        let le = utf16le_bytes("Hello, World! This is a longer line for the heuristic.");
        let d = detect(&le);
        assert_eq!(d.encoding, Encoding::Utf16Le, "got {:?}", d.encoding);
        assert_eq!(d.method, DetectionMethod::NulPattern);
        assert_eq!(d.confidence, Confidence::Medium);

        let be = utf16be_bytes("Hello, World! This is a longer line for the heuristic.");
        let d = detect(&be);
        assert_eq!(d.encoding, Encoding::Utf16Be);
        assert_eq!(d.method, DetectionMethod::NulPattern);
    }

    #[test]
    fn nul_pattern_detects_bomless_utf32() {
        let le = utf32le_bytes("Hello, World! This is a longer line for the heuristic.");
        let d = detect(&le);
        assert_eq!(d.encoding, Encoding::Utf32Le, "got {:?}", d.encoding);

        let be = utf32be_bytes("Hello, World! This is a longer line for the heuristic.");
        let d = detect(&be);
        assert_eq!(d.encoding, Encoding::Utf32Be);
    }

    #[test]
    fn nul_pattern_prefers_utf32_over_utf16_on_ties() {
        // UTF-32LE ASCII also has NULs at every odd byte; the stricter
        // pattern must win.
        let le32 = utf32le_bytes("aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa");
        assert_eq!(detect(&le32).encoding, Encoding::Utf32Le);
        // And UTF-16LE ASCII must NOT be misread as UTF-32.
        let le16 = utf16le_bytes("aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa");
        assert_eq!(detect(&le16).encoding, Encoding::Utf16Le);
    }

    #[test]
    fn nul_pattern_ignores_scattered_binary_nuls() {
        // Binary with scattered NULs: no positional structure -> fallback.
        let mut bytes = Vec::new();
        for i in 0..2000u32 {
            bytes.push((i.wrapping_mul(2654435761) >> 16) as u8);
            if i % 5 == 0 {
                bytes.push(0);
            }
        }
        let d = detect(&bytes);
        assert_ne!(d.method, DetectionMethod::NulPattern);
        assert_eq!(d.encoding, Encoding::Windows1252);
    }

    #[test]
    fn nul_pattern_cjk_limitation_is_honest() {
        // UTF-16LE CJK text has no NUL pattern. We report an honest Low
        // fallback rather than guessing — documented limitation.
        let cjk = utf16le_bytes("日本語テスト日本語テスト日本語テスト");
        let d = detect(&cjk);
        assert_eq!(d.encoding, Encoding::Windows1252);
        assert_eq!(d.confidence, Confidence::Low);
    }

    // ---------------- decode: adversarial ----------------

    #[test]
    fn invalid_utf8_at_every_boundary_position() {
        let base = "The quick brown fox jumps over the lazy dog 日本語";
        let base_bytes = base.as_bytes();
        // Insert each poison at every byte position: never panics, and
        // detection (no NULs, invalid UTF-8) lands on the safe fallback.
        let poisons: &[&[u8]] = &[b"\xFF", b"\x80", b"\xC3", b"\xE2\x82", b"\xF0\x9F"];
        for poison in poisons {
            for pos in 0..=base_bytes.len() {
                let mut bytes = Vec::with_capacity(base_bytes.len() + poison.len());
                bytes.extend_from_slice(&base_bytes[..pos]);
                bytes.extend_from_slice(poison);
                bytes.extend_from_slice(&base_bytes[pos..]);
                // A poison can fuse with neighboring bytes into valid UTF-8
                // (e.g. E2 82 + A5 = U+2025); detection must follow the
                // evidence either way, and never panic. (No content
                // assertion: a poison landing inside a word legitimately
                // breaks it; panic-freedom and evidence-following are the
                // properties under test.)
                let d = detect(&bytes);
                if std::str::from_utf8(&bytes).is_ok() {
                    assert_eq!(d.encoding, Encoding::Utf8, "pos {pos}");
                } else {
                    assert_eq!(d.encoding, Encoding::Windows1252, "pos {pos}");
                }
                let _ = decode(&bytes, d.encoding);
            }
        }
    }

    #[test]
    fn overlong_encodings_are_invalid_utf8() {
        // Overlong forms must not decode as the intended char.
        for seq in [&b"\xC0\xAF"[..], b"\xE0\x80\xAF", b"\xF0\x80\x80\xAF"] {
            assert!(std::str::from_utf8(seq).is_err());
            let d = detect(seq);
            assert_eq!(d.encoding, Encoding::Windows1252);
            let (_, had) = decode(seq, Encoding::Utf8);
            assert!(had.has_errors());
        }
    }

    #[test]
    fn lone_surrogates_in_utf16_never_panic() {
        // Lone surrogates are malformed: U+FFFD + reported, never a panic.
        for unit in [0xD800u16, 0xDBFF, 0xDC00, 0xDFFF] {
            for le in [true, false] {
                let bytes = if le {
                    unit.to_le_bytes().to_vec()
                } else {
                    unit.to_be_bytes().to_vec()
                };
                let (text, had) = decode(
                    &bytes,
                    if le {
                        Encoding::Utf16Le
                    } else {
                        Encoding::Utf16Be
                    },
                );
                assert!(had.has_errors(), "unit {unit:04x}");
                assert_eq!(text, "\u{FFFD}");
            }
        }
        // Surrogate *pairs* are well-formed and round-trip byte-exact.
        let text = "A\u{1F600}Z";
        for (bytes, enc) in [
            (utf16le_bytes(text), Encoding::Utf16Le),
            (utf16be_bytes(text), Encoding::Utf16Be),
        ] {
            let (decoded, had) = decode(&bytes, enc);
            assert!(!had.has_errors());
            assert_eq!(decoded, text);
            assert_eq!(encode(&decoded, &explicit(enc)), bytes);
        }
    }

    #[test]
    fn truncated_utf16_tail_reports_error() {
        let (text, had) = decode(b"h\x00i", Encoding::Utf16Le);
        assert!(had.has_errors());
        assert!(text.starts_with('h'));
    }

    #[test]
    fn truncated_utf32_tail_and_bad_scalars_report_error() {
        // 1-3 leftover bytes.
        for tail in [b"a\x00\x00".as_slice(), b"a\x00\x00\x00\x00", b""] {
            let (text, had) = decode(tail, Encoding::Utf32Le);
            if tail.is_empty() {
                assert!(!had.has_errors());
                assert_eq!(text, "");
            } else {
                assert!(had.has_errors(), "tail {tail:02x?}");
            }
        }
        // Surrogate value and out-of-range value in UTF-32.
        for v in [0xD800u32, 0xDFFF, 0x110000, 0xFFFF_FFFF] {
            let bytes = v.to_le_bytes();
            let (text, had) = decode(&bytes, Encoding::Utf32Le);
            assert!(had.has_errors(), "scalar {v:08x}");
            assert_eq!(text, "\u{FFFD}");
        }
        // Largest valid scalar round-trips.
        let bytes = 0x10FFFFu32.to_be_bytes();
        let (text, had) = decode(&bytes, Encoding::Utf32Be);
        assert!(!had.has_errors());
        assert_eq!(text, "\u{10FFFF}");
        assert_eq!(encode(&text, &explicit(Encoding::Utf32Be)), bytes);
    }

    #[test]
    fn nul_bytes_do_not_panic_anywhere() {
        // NULs at every position of tiny inputs, all encodings: decode and
        // detect must never panic (char counts differ per unit width, so
        // only panic-freedom is asserted here).
        for len in 0..8usize {
            let bytes = vec![0u8; len];
            for enc in Encoding::all() {
                let _ = decode(&bytes, *enc);
            }
            let _ = detect(&bytes);
        }
    }

    // ---------------- round-trip properties ----------------

    #[test]
    fn roundtrip_utf8_with_and_without_bom() {
        let text = "Hello, 世界! 🎉\nLine2";
        for with_bom in [false, true] {
            let mut bytes = Vec::new();
            if with_bom {
                bytes.extend_from_slice(Bom::Utf8.bytes());
            }
            bytes.extend_from_slice(text.as_bytes());
            let (decoded, detected, had) = decode_detected(&bytes);
            assert_eq!(decoded, text);
            assert!(!had.has_errors());
            assert_eq!(detected.bom, with_bom.then_some(Bom::Utf8));
            assert_eq!(encode(&decoded, &detected), bytes);
        }
    }

    #[test]
    fn roundtrip_utf16_with_and_without_bom() {
        let text = "Hello, 世界! 🎉";
        for (make, enc, bom) in [
            (utf16le_bytes as fn(&str) -> Vec<u8>, Encoding::Utf16Le, Bom::Utf16Le),
            (utf16be_bytes as fn(&str) -> Vec<u8>, Encoding::Utf16Be, Bom::Utf16Be),
        ] {
            for with_bom in [false, true] {
                let mut bytes = Vec::new();
                if with_bom {
                    bytes.extend_from_slice(bom.bytes());
                }
                bytes.extend_from_slice(&make(text));
                let (decoded, detected, had) = decode_detected(&bytes);
                assert_eq!(decoded, text);
                assert!(!had.has_errors());
                assert_eq!(detected.encoding, enc);
                assert_eq!(encode(&decoded, &detected), bytes);
            }
        }
    }

    #[test]
    fn roundtrip_utf32_with_and_without_bom() {
        let text = "Hello, 世界! 🎉";
        for (make, enc, bom) in [
            (utf32le_bytes as fn(&str) -> Vec<u8>, Encoding::Utf32Le, Bom::Utf32Le),
            (utf32be_bytes as fn(&str) -> Vec<u8>, Encoding::Utf32Be, Bom::Utf32Be),
        ] {
            for with_bom in [false, true] {
                let mut bytes = Vec::new();
                if with_bom {
                    bytes.extend_from_slice(bom.bytes());
                }
                bytes.extend_from_slice(&make(text));
                let (decoded, detected, had) = decode_detected(&bytes);
                assert_eq!(decoded, text);
                assert!(!had.has_errors());
                assert_eq!(detected.encoding, enc);
                assert_eq!(encode(&decoded, &detected), bytes);
            }
        }
    }

    #[test]
    fn roundtrip_every_byte_of_every_single_byte_encoding() {
        // Property: for every single-byte encoding and every byte value,
        // decoding never panics; when the byte is *defined* (no error),
        // decode->encode is byte-exact.
        for enc in Encoding::all().iter().filter(|e| e.is_single_byte()) {
            let det = explicit(*enc);
            let mut defined = 0u32;
            for b in 0..=255u8 {
                let (text, had) = decode(&[b], *enc);
                assert_eq!(text.chars().count(), 1, "{enc:?} byte {b:02x}");
                if !had.has_errors() {
                    defined += 1;
                    assert_eq!(
                        encode(&text, &det),
                        vec![b],
                        "{enc:?} byte {b:02x} must round-trip"
                    );
                }
            }
            // Every single-byte encoding defines the full ASCII range.
            assert!(defined >= 128, "{enc:?} defines only {defined} bytes");
        }
    }

    #[test]
    fn roundtrip_multibyte_encodings() {
        let strings = [
            "Hello, World!",
            "caf\u{E9} \u{20AC}",
            "\u{65E5}\u{672C}\u{8A9E}\u{30C6}\u{30B9}\u{30C8}",
            "\u{D55C}\u{AD6D}\u{C5B4}",
            "\u{4E2D}\u{6587}\u{6D4B}\u{8BD5}",
            "\u{041F}\u{0440}\u{0438}\u{0432}\u{0435}\u{0442}",
        ];
        for enc in [
            Encoding::ShiftJis,
            Encoding::EucJp,
            Encoding::EucKr,
            Encoding::Big5,
            Encoding::Gb18030,
            Encoding::Gbk,
            Encoding::Iso2022Jp,
        ] {
            let det = explicit(enc);
            for s in strings {
                let (bytes, had) = encode_checked(s, &det);
                if had.has_errors() {
                    continue; // unmappable for this encoding: not well-formed input
                }
                let (decoded, had2) = decode(&bytes, enc);
                assert!(!had2.has_errors(), "{enc:?} {s:?}");
                assert_eq!(decoded, s, "{enc:?} {s:?}");
                assert_eq!(encode(&decoded, &det), bytes, "{enc:?} {s:?}");
            }
        }
        // GB18030 is total over Unicode: everything round-trips.
        let det = explicit(Encoding::Gb18030);
        for s in strings.iter().copied().chain(["\u{1F600}"]) {
            let (bytes, had) = encode_checked(s, &det);
            assert!(!had.has_errors());
            let (decoded, _) = decode(&bytes, Encoding::Gb18030);
            assert_eq!(&decoded, s);
        }
    }

    #[test]
    fn decode_strips_single_bom_like_textdecoder() {
        let (text, _) = decode(b"\xEF\xBB\xBFhi", Encoding::Utf8);
        assert_eq!(text, "hi");
        // A genuine second BOM char is data, not stripped.
        let (text, _) = decode(b"\xEF\xBB\xBF\xEF\xBB\xBFhi", Encoding::Utf8);
        assert_eq!(text, "\u{FEFF}hi");
    }

    // ---------------- labels ----------------

    #[test]
    fn from_label_aliases() {
        assert_eq!(Encoding::from_label("latin1"), Some(Encoding::Windows1252));
        assert_eq!(
            Encoding::from_label("iso-8859-1"),
            Some(Encoding::Windows1252)
        );
        // encoding_rs keeps GBK as its own encoding (same decoder as
        // GB18030, slightly different encoder): mapped exactly, not merged.
        assert_eq!(Encoding::from_label("gbk"), Some(Encoding::Gbk));
        assert_eq!(Encoding::from_label("gb18030"), Some(Encoding::Gb18030));
        assert_eq!(Encoding::from_label("UTF-16LE"), Some(Encoding::Utf16Le));
        assert_eq!(Encoding::from_label("utf-32be"), Some(Encoding::Utf32Be));
        assert_eq!(Encoding::from_label("shift_jis"), Some(Encoding::ShiftJis));
        assert_eq!(Encoding::from_label("nope-not-an-encoding"), None);
        // "replacement" is a WHATWG encoding with no erax counterpart.
        assert_eq!(Encoding::from_label("replacement"), None);
    }

    #[test]
    fn label_roundtrip_covers_all_encodings() {
        assert_eq!(Encoding::all().len(), 40);
        for enc in Encoding::all() {
            assert_eq!(
                Encoding::from_label(enc.label()),
                Some(*enc),
                "label {}",
                enc.label()
            );
        }
    }

    // ---------------- locale ----------------

    #[test]
    fn locale_hint_parsing() {
        assert_eq!(
            locale_hint_from_str("ru_RU.UTF-8").preferred,
            Some(Encoding::Windows1251)
        );
        assert_eq!(
            locale_hint_from_str("ja_JP.eucJP").preferred,
            Some(Encoding::ShiftJis)
        );
        assert_eq!(
            locale_hint_from_str("zh_TW.UTF-8").preferred,
            Some(Encoding::Big5)
        );
        assert_eq!(
            locale_hint_from_str("zh_CN.GB18030").preferred,
            Some(Encoding::Gb18030)
        );
        assert_eq!(locale_hint_from_str("en_US.UTF-8").preferred, None);
        assert_eq!(locale_hint_from_str("C").preferred, None);
        assert_eq!(locale_hint_from_str("").preferred, None);
        assert_eq!(
            locale_hint_from_str("de_DE@euro").language.as_deref(),
            Some("de")
        );
    }

    #[test]
    fn locale_hint_only_replaces_fallback_never_evidence() {
        let hint = LocaleHint {
            preferred: Some(Encoding::Windows1251),
            language: Some("ru".to_string()),
        };
        // BOM evidence wins over the hint.
        let d = detect_with_hint(b"\xFF\xFEh\x00", &hint);
        assert_eq!(d.encoding, Encoding::Utf16Le);
        assert_eq!(d.method, DetectionMethod::Bom);
        // UTF-8 evidence wins over the hint.
        let d = detect_with_hint("plain ascii".as_bytes(), &hint);
        assert_eq!(d.encoding, Encoding::Utf8);
        // Ambiguous bytes: hint applies, honestly labeled Low.
        let d = detect_with_hint(b"\xCF\xF0\xE8\xE2\xE5\xF2", &hint);
        assert_eq!(d.encoding, Encoding::Windows1251);
        assert_eq!(d.method, DetectionMethod::LocaleHint);
        assert_eq!(d.confidence, Confidence::Low);
        // Without a hint: the plain fallback.
        let d = detect(b"\xCF\xF0\xE8\xE2\xE5\xF2");
        assert_eq!(d.encoding, Encoding::Windows1252);
        assert_eq!(d.method, DetectionMethod::Fallback);
    }

    // ---------------- encode errors ----------------

    #[test]
    fn encode_checked_reports_unmappable() {
        let det = explicit(Encoding::Windows1252);
        let (bytes, had) = encode_checked("A\u{1F600}", &det);
        assert!(had.has_errors(), "emoji is unmappable in windows-1252");
        assert!(!bytes.is_empty());
        // UTF-8 maps everything.
        let (_, had) = encode_checked("A\u{1F600}", &explicit(Encoding::Utf8));
        assert!(!had.has_errors());
    }

    // ---------------- buffer integration ----------------

    #[test]
    fn buffer_from_bytes_tags_encoding_and_preserves() {
        let mut bytes = Bom::Utf16Le.bytes().to_vec();
        bytes.extend_from_slice(&utf16le_bytes("héllo"));
        let loaded = buffer_from_bytes(PathBuf::from("x.txt"), bytes.clone());
        assert_eq!(loaded.detected.encoding, Encoding::Utf16Le);
        assert_eq!(loaded.detected.bom, Some(Bom::Utf16Le));
        assert!(!loaded.had_errors);
        assert!(loaded.byte_exact);
        assert_eq!(loaded.buffer.content_string(), "héllo");
        assert_eq!(loaded.buffer.filename, Some(PathBuf::from("x.txt")));
        assert_eq!(loaded.buffer.original_bytes, Some(bytes));
    }

    #[test]
    fn buffer_from_bytes_plain_utf8_needs_no_preservation() {
        let loaded = buffer_from_bytes(PathBuf::from("x.txt"), b"plain".to_vec());
        assert_eq!(loaded.detected, DetectedEncoding::utf8());
        assert!(!loaded.byte_exact);
        assert_eq!(loaded.buffer.original_bytes, None);
        assert_eq!(loaded.buffer.content_string(), "plain");
    }

    #[test]
    fn load_file_with_encoding_end_to_end() {
        let dir = tempfile::tempdir().expect("tempdir");
        // UTF-16LE with BOM.
        let p1 = dir.path().join("utf16.txt");
        let mut raw1 = Bom::Utf16Le.bytes().to_vec();
        raw1.extend_from_slice(&utf16le_bytes("hello wörld"));
        std::fs::write(&p1, &raw1).expect("write");
        // Raw binary garbage (invalid UTF-8, no structure).
        let p2 = dir.path().join("binary.dat");
        let raw2: Vec<u8> = (0..4096u32).map(|i| (i.wrapping_mul(2654435761) >> 13) as u8).collect();
        assert!(std::str::from_utf8(&raw2).is_err());
        std::fs::write(&p2, &raw2).expect("write");
        // UTF-8 with BOM.
        let p3 = dir.path().join("bom.txt");
        let raw3 = b"\xEF\xBB\xBFbommed".to_vec();
        std::fs::write(&p3, &raw3).expect("write");

        let mut l1 = load_file_with_encoding(&p1).expect("load utf16");
        assert_eq!(l1.detected.encoding, Encoding::Utf16Le);
        assert_eq!(l1.buffer.content_string(), "hello wörld");
        // Unmodified save is byte-exact.
        let out1 = dir.path().join("out1.txt");
        l1.buffer.save_as(&out1).expect("save");
        assert_eq!(std::fs::read(&out1).expect("read"), raw1);

        let mut l2 = load_file_with_encoding(&p2).expect("load binary");
        assert_eq!(l2.detected.encoding, Encoding::Windows1252);
        assert_eq!(l2.detected.confidence, Confidence::Low);
        let out2 = dir.path().join("out2.dat");
        l2.buffer.save_as(&out2).expect("save");
        assert_eq!(std::fs::read(&out2).expect("read"), raw2);

        let mut l3 = load_file_with_encoding(&p3).expect("load bom");
        assert_eq!(l3.detected.bom, Some(Bom::Utf8));
        assert_eq!(l3.buffer.content_string(), "\u{FEFF}bommed");
        let out3 = dir.path().join("out3.txt");
        l3.buffer.save_as(&out3).expect("save");
        assert_eq!(std::fs::read(&out3).expect("read"), raw3);

        // IO errors still pass through typed.
        assert!(matches!(
            load_file_with_encoding(dir.path().join("missing.txt")),
            Err(LoadError::InvalidPath(_))
        ));
    }

    #[test]
    fn large_single_line_exercises_sampled_detection() {
        // 8 MiB single line: detection must stay O(sample), decode is linear.
        let big = vec![b'a'; 8 * 1024 * 1024];
        let d = detect(&big);
        assert_eq!(d.encoding, Encoding::Utf8);
        let (text, _, had) = decode_detected(&big);
        assert!(!had.has_errors());
        assert_eq!(text.len(), big.len());

        // 4 MiB of BOM-less UTF-16LE 'a's: exercises the 32 KiB sample cap.
        let mut wide = Vec::with_capacity(4 * 1024 * 1024);
        for _ in 0..(2 * 1024 * 1024) {
            wide.extend_from_slice(b"a\x00");
        }
        let d = detect(&wide);
        assert_eq!(d.encoding, Encoding::Utf16Le);
        assert_eq!(d.method, DetectionMethod::NulPattern);
    }
}
