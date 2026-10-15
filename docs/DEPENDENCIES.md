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
