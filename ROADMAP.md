# erax Development Roadmap

A text editor under active development.

**License**: GPL-2.0-only

---

## Current Status (2025-12-21)

| Component | Status | Notes |
|-----------|--------|-------|
| Core Engine | ✅ Working | Rope-based buffer, undo/redo, UTF-8 |
| Terminal Mode | ✅ Working | crossterm, multi-window, mouse support |
| Sed Mode | ✅ Working | Basic POSIX stream editing |
| GUI Mode | 🚧 Experimental | WGPU renderer, delegates input to TUI |

---

## Completed

### Core Engine
- [x] Rope-based buffer storage (`ropey`)
- [x] Undo/Redo with smart grouping
- [x] UTF-8 grapheme cluster support
- [x] Lossy loading for files with invalid UTF-8

### Terminal Mode
- [x] Interactive TUI with `crossterm`
- [x] Adaptive rendering (ASCII, ANSI, UTF-8)
- [x] Incremental rendering via DirtyTracker
- [x] Multi-key sequences (^X, ESC prefixes)
- [x] Window layouts (vsplit/hsplit)
- [x] Mouse click, drag, scroll
- [x] Menu bar with keyboard/mouse navigation

### Sed Mode
- [x] Stream editing via `-e` and `-f` flags
- [x] Atomic in-place editing

### GUI Mode
- [x] GPU rendering with `wgpu`
- [x] Glyph atlas
- [x] Input delegation to TUI (unified input flow)

### Other
- [x] Rule-based syntax highlighting (C, Rust, Python, Go, JS. Will be working to polish the algorithm for improved accuracy and generalization in the near future.)
- [x] Programmer's calculator
- [x] Macro recording/playback (Untested.)

---

## Planned

### Near-term
- [ ] GUI polish (font fallback, scrolling)
- [ ] More syntax highlighting languages
- [ ] Buffer performance optimization, including better support for lossy loading of files with invalid UTF-8, and better font fallback loading performance for GUI mode in files with multiple exotic character sets.

### Future
- [ ] Streaming search
- [ ] Command palette

---

## Technology

- **Language**: Rust (Edition 2024)
- **Text Engine**: `ropey`
- **TUI**: `crossterm`
- **GUI**: `wgpu`, `ab_glyph`