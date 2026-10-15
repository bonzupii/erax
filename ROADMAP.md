# erax Development Roadmap

A text editor under active development.

**License**: GPL-2.0-only

---

## Current Status (2026-10-14)

| Component | Status | Notes |
|-----------|--------|-------|
| Core Engine | ✅ Working | Rope-based buffer, undo/redo, UTF-8 (in-house B-tree DS planned) |
| Terminal Mode | ✅ Working | crossterm, multi-window, mouse support, mode-aware UI |
| Input Models | ✅ Working | KeyTrie (default), Modal (Vim), Standard (CUA) — compile-time const |
| Sed Mode | ✅ Working | Stream editing + diff mode |
| GUI Mode | 🚧 Experimental | wgpu renderer, dumb passthrough for the TUI, complex-script shaping |
| Printing | ✅ Working | Cross-platform via `printers` crate, typed errors |
| Encodings/Hex | ✅ Working | 40 encodings, BOM detection, byte-exact round-trips, hex viewer |

---

## Completed

### Core Engine
- [x] Rope-based buffer storage (`ropey`; in-house B-tree replacement designed, pending)
- [x] Undo/Redo with smart grouping
- [x] UTF-8 grapheme cluster support
- [x] Encoding layer: detection, UTF-16/32, ISO-8859/Windows code pages, byte-exact round-trips, adversarial-tested

### Terminal Mode
- [x] Interactive TUI with `crossterm`
- [x] Adaptive rendering (ASCII, ANSI, UTF-8)
- [x] Incremental rendering via DirtyTracker
- [x] Multi-key sequences (^X, ESC prefixes)
- [x] Window layouts (vsplit/hsplit)
- [x] Mouse click, drag, scroll
- [x] Menu bar with keyboard/mouse navigation
- [x] Mode-aware prompt/status bar (per-model segments, `:` command line, per-mode cursor shapes via DECSCUSR)

### Input Models
- [x] Typed compile-time keymaps (`KEYBINDINGS.md` generated from source)
- [x] KeyTrie: classic uEmacs chords
- [x] Modal: real Vim semantics (counts, operators over motions, registers, dot-repeat, visual, command line)
- [x] Standard: CUA bindings, clipboard vs kill-ring separation, dual redo, smart Home

### Sed Mode
- [x] Stream editing via `-e` and `-f` flags
- [x] Atomic in-place editing
- [x] Full diff engine (unified/context output)

### GUI Mode
- [x] GPU rendering with `wgpu` 30
- [x] Glyph atlas
- [x] Input delegation to TUI (unified input flow); GUI owns no editor state
- [x] Complex-script shaping via cosmic-text runs (Indic, Khmer, RTL/BiDi, CJK, emoji) with caret mapping through clusters

### Other
- [x] Rule-based syntax highlighting (C, Rust, Python, Go, JS)
- [x] Programmer's calculator
- [x] Macro recording/playback (tested)
- [x] Diagnostics pane (GCC/Rust/Make parsers, dedup)
- [x] Spell checker (shared dictionary, real suggestions)
- [x] Snippets (Unicode-safe, tab stops + mirroring)
- [x] Real printing (no fake success)

---

## Planned

### Near-term
- [ ] In-house B-tree data structure replacing `ropey` (design settled; includes pager vs mmap benchmark)
- [ ] WebUI: server mode + wasm web shell as a child of the GUI (typed versioned frame protocol, opt-in, 127.0.0.1 + per-session token)
- [ ] Release opt-level decision (benchmark z vs 3 vs 2 vs s + assembly inspection)
- [ ] Dependency reduction (in-house replacements where provably as correct and fast)
- [ ] Adversarial test program (`tests/adversarial.rs`): malformed inputs, truncation, pathological regexes, undo storms, huge vim counts
- [ ] Legacy validation sweep (L-series): every pristine feature gets behavior tests

### Future
- [ ] Streaming search
- [ ] Command palette
- [ ] Built-in LLM assistant (final phase; design doc first): OpenAI-compatible endpoint, static-const config, untrusted-actor permission model, scripted mock test fixture
- [ ] Competitive benchmarks vs vim/emacs/Sublime/Zed on settled release configuration

---

## Technology

- **Language**: Rust (Edition 2024)
- **Text Engine**: `ropey` (slated for in-house B-tree replacement)
- **TUI**: `crossterm`
- **GUI**: `wgpu` 30, `cosmic-text` 0.19 (shaping), `swash`
- **Printing**: `printers`
- **Encodings**: `encoding_rs` + in-house UTF-32/UTF-16 codecs