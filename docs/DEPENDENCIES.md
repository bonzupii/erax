# erax dependency inventory

Owner direction (Bonzupii, 2026-10-14): work toward zero external dependencies
where feasible (N-266 stays pinned as the terminal goal, not this patch's
required end state). Every crate below is an interim delegation: use the crate
at the abstraction it provides, replace trivial ones with in-house code where
the replacement is provably as correct and as fast, and vendor
reference implementations (with license care — erax is GPL-2.0-only) when a
crate is selected for hand-rolled replacement.

Upgrade policy (D1, 2026-10-14): all dependencies at latest stable, majors
included. Upgrade log: 2026-10-14 — wgpu 27.0.1→30.0.1, cosmic-text 0.15.0→0.19.0,
similar 2.7.0→3.2.0, criterion 0.5.1→0.8.2 (dev), printers added at 2.3.0;
ropey/pollster/rustc-hash held (slated for replacement — no upgrade effort).
Cargo.lock: 354 packages.

## Keep — high-impact subsystems (replace only with proven in-house equivalents)

| crate | version | why it's here | replaceability |
|---|---|---|---|
| wgpu | 30.0.1 | GPU renderer (native shell + future wasm web shell) | No — GPU abstraction is a high-impact subsystem |
| winit | 0.30.13 | native window/event loop | No — platform windowing |
| cosmic-text | 0.19.0 | font shaping, layout, rasterization (N-120/N-259/N-260) | No — text shaping is a high-impact subsystem |
| regex | 1.13.1 | sed patterns, search | No — correct regex engines are hard; differential-test before any in-house attempt |
| crossterm | 0.29.0 | terminal I/O, events, cursor styles | Unlikely — terminal handling is platform-fiddly |
| clap | 4.6.7 | CLI parsing (derive) | Low priority — small but correct; evaluate later |
| vte | 0.15.0 | terminal emulation (terminal_host) | Keep while terminal path exists |
| portable-pty | 0.9.0 | pty for terminal emulation | Keep while terminal path exists |
| unicode-segmentation | 1.13.3 | grapheme/word boundaries | Unlikely — Unicode tables |
| unicode-width | 0.2.2 | display width | Unlikely — Unicode tables |
| bytemuck | 1.21 | Pod/Zeroable for GPU buffers | Low priority — tiny, sound |
| tempfile | 3.15 | tests + print-to-file fallback | Low priority |
| similar | 3.2.0 | diff/hunk computation (sed review) | Keep — diff is algorithmic; in-house must beat it on tests+benches first |
| printers | 2.3.0 | cross-platform printing (N-267) | Interim per N-267 → N-266 path |

## Speech backend (optional `speech` cargo feature; off by default)

Enabled only when the `SPEECH_BACKEND` config const is not `Off`.
All crates below are GPL-2.0-compatible (taken under their MIT leg
where dual-licensed). The full 137-crate transitive closure was
audited 2026-10-16: no GPL-3.0/AGPL anywhere, and no espeak-ng code
in the tree (the phonemizer is misaki-rs with its espeak fallback
disabled; see the session 26 PROGRESS entry for the audit table).

| crate | version | license (leg taken) | why it's here |
|---|---|---|---|
| whisper-rs | =0.16.0 | Unlicense | Safe bindings to whisper.cpp for the built-in STT (Whisper large-v3-turbo q5_0) |
| floravox-core | =0.8.8 | MIT (of Apache-2.0 OR MIT) | Pure-Rust TTS: G2P orchestration, SSML, word timings; drives our embedded-weights `VoiceBackend` impl |
| floravox-g2p | =0.8.8 | MIT (of Apache-2.0 OR MIT) | misaki-rs wrapper: Rust-native English/Japanese G2P (dictionaries compiled in) |
| ort | =2.0.0-rc.14 | MIT (of MIT OR Apache-2.0) | ONNX Runtime inference for the embedded Kokoro-82M int8 model (pinned to floravox's version) |
| anyhow | 1 | MIT (of MIT OR Apache-2.0) | Error type for the `VoiceBackend` trait impl |
| sha2 | 0.10 | MIT (of MIT OR Apache-2.0) | build-dependency: sha256 verification of provisioned weights in build.rs |
| misaki-rs | 0.3.0 | MIT | (transitive via floravox-g2p) Rust-native phonemizer; no runtime file reads |
| language-tokenizer | 0.1.0 | WTFPL (per its GitHub repo LICENSE.md; the crates.io metadata has no license field) | (transitive via misaki-rs) tokenizer |
| r-efi | 6.0.0 | MIT (of MIT OR Apache-2.0 OR LGPL-2.1-or-later) | (transitive) UEFI types, pulled via the crypto stack |

## Replace — slated for in-house (no upgrade effort spent)

| crate | version | plan | status |
|---|---|---|---|
| ropey | 1.6.1 (held) | in-house B-tree piece table (DATA-STRUCTURE.md); unsafe only in DS module | design settled, implementation queued |
| pollster | 0.4.0 (held) | in-house `block_on` (~15 lines; single use in gui/renderer.rs) | queued |
| rustc-hash | 2.1.3/1.1.0 (held) | evaluate: vendor FxHashMap reference exactly (D2/D3) or replace with in-house; used in gui/atlas.rs, gui/renderer.rs | queued |

## Dev-only

| crate | version | why |
|---|---|---|
| criterion | 0.8.2 | benchmark harness (never ships) |

## Vendored code (D2/D3/D4)

None yet. When code is vendored: record here the source crate+version, its
license (embed under the MIT leg for dual MIT/Apache-2.0; Apache-2.0-only →
flag for main agent, do not embed), preserve copyright notices in the vendored
files, and list origins in `NOTICE` (repo root). Every vendored `unsafe` site
is inventoried in ~/workspace/erax-work/VERIFICATION.md with the original
author's safety argument and our proof artifacts.

License check for the one pending addition: `encoding_rs` (if adopted for the
encoding batch) is dual MIT/Apache-2.0 — acceptable; record here when added.
