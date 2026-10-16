# EraX

A text editor under active development, written in Rust.

EraX is a multi-modal text editor with GUI, TUI, and stream editing modes. It ships three input models (compile-time selected): KeyTrie (classic uEmacs-style chords, the default), Modal (real Vim semantics: Normal/Insert/Visual/OperatorPending/CommandLine), and Standard (Notepad/VS Code feel: CUA bindings, selection-first editing). Configuration is a set of typed `pub const`s compiled into the binary — invalid configuration is a compile error, not a runtime surprise.

**Status**: Alpha (v0.8.0-alpha). Many features are incomplete or experimental. Risk of editor malfunction and data loss are high if you choose to use the editor in this stage.

## License

GNU General Public License, version 2 only (GPL-2.0-only).

## Features

- **Terminal mode (TUI)**: Interactive editing with `crossterm`. Supports ASCII, ANSI, and UTF-8 terminals. Also has GPM mouse support for the 3 people in the world who actually use GPM.
- **Graphical mode (GUI)**: GPU-accelerated rendering via `wgpu` and `winit`.
- **Stream mode (Sed)**: Basic POSIX-style stream editing for scripting. Also supports "diff-mode" for viewing and approving diffs for sed-style edit commands.
- **Syntax highlighting**: Rule-based lexer for C, Rust, Python, Go, JavaScript. (no heavy tree-sitter! The highlighting may not be "semantically correct" but it looks pretty and makes code more readable for the 99% of people who don't care about semantic correctness of syntax highlighting.)
- **uEmacs-style keybindings**: Common navigation and editing commands.
- **Three input models** (compile-time `InputModel` const; see `KEYBINDINGS.md`):
  - **KeyTrie** (default): classic uEmacs chords and prefix sequences.
  - **Modal**: real Vim modal semantics — Normal/Insert/Visual/Visual-Line/OperatorPending/CommandLine, counts (`3dw`, `d3w`), `d`/`y`/`c` over motions, registers, `.` repeat, `:` command line, `/` search. Mode-aware status bar and per-mode cursor shapes.
  - **Standard**: modeless Notepad/VS Code feel — typing inserts, `Ctrl+C/X/V` clipboard (separate from the kill ring), `Ctrl+Z`/`Ctrl+Y` + `Ctrl+Shift+Z` redo, smart Home, Shift+arrow selection, selection readout in the status bar.
- **Diagnostics pane**: parses GCC/Clang, Rust (multi-line records merged, warnings stay warnings), and Make output; duplicates merged; jump-to-location.
- **Spell checking**: one shared static dictionary; Unicode case-folded lookup; Damerau–Levenshtein suggestions (`teh` → `the`).
- **Snippets**: Unicode-safe expansion, real tab stops with mirroring (`${1:default}`), typed const registry (Rust/C/Python built-ins).
- **Printing**: real cross-platform printing via the `printers` crate (CUPS/WinSpool); typed errors — a failed print is reported, never silently "succeeded".
- **Encodings + hex viewer**: BOM detection, UTF-16/32, ISO-8859/Windows code pages via `encoding_rs` + in-house codecs; byte-exact round-trips; read-only-by-default hex view with warn-on-edit.
- **Complex-script GUI shaping**: per-run shaping (accents/IPA, combining stacks, ligatures, Indic, Khmer, RTL/BiDi, CJK, emoji) with caret mapping through grapheme clusters.
- **Multi-window**: Basic split window support (vsplit/hsplit).
- **Compile time configuration**: Configs are parsed and validated at compilation, and compiled into the binary. This reduces runtime clutter and config parsing overhead. Some settings may be available to change on a per-session basis but for the most part you have to re-compile to change your settings. This is a feature. PRs requesting any sort of text based runtime configuration will be ignored. If you want that as a feature fork it and add it yourself.

## Building

Requires Rust (stable, Edition 2024).

```bash
# Release build
cargo build --release
```

## Usage

```bash
# Edit a file in the terminal
erax -u file.txt

# Stream editing
echo "old text" | erax -e 's/old/new/'

# GUI mode (if built with gui feature, this will run as the default mode unless it detects that your environment is incapable of graphics)
erax file.txt
```

### Default frontend (compile-time)

Which interface starts when no CLI flag selects one is a typed `pub const`
(`src/config/frontend.rs`): `SHELL_MODE` is `Tui`, `Utf8`, or `WebUi`.
Explicit flags (`-g`, `-u`, `-a`, `-s`, `--web-server`) always override it.
The same module holds the WebUI server defaults: `WEB_BIND`
(`"127.0.0.1:8080"`, overridden by `--web-bind`) and `WEB_ALLOW_REMOTE`
(bakes the non-loopback acknowledgement into the binary; the
`--web-allow-remote` flag ORs with it).

### WebUI (browser frontend, VS Code-server style)

One command. The server prints the URL **and the per-session token** — the
token is required; there is no unauthenticated mode.

```bash
erax --web-server
# erax web server listening on 127.0.0.1:8080
# token: <32 hex chars>
# open: http://127.0.0.1:8080/?token=<32 hex chars>
```

Open the printed `open:` URL in a browser. That's it — the page, the wasm
client, and the font are all served by the same command. The client files
(`web-assets/`) are embedded in the server binary, so this works from any
install layout; `ERAX_WEB_ASSETS_DIR` (when set) overrides them with files
from disk for web dev iteration without recompiling the server.

**Building the wasm client (release):** the checked-in `web-assets/` are
built reproducibly from `web-shell/` (its own crate; the presentation core
and protocol are the same sources as native, included via `#[path]` —
`web-shell/src/config` includes `src/config/font.rs`,
`src/config/cursor_style.rs`, `src/config/scrollbar.rs`,
`src/config/theme.rs`, and `src/config/effects.rs` directly, so the web
shell cannot drift from the native config). Exact commands:

```bash
cd web-shell
cargo build --release --target wasm32-unknown-unknown
# → target/wasm32-unknown-unknown/release/erax_web_shell.wasm
wasm-bindgen --target web --out-dir /tmp/wbg --out-name erax-web-shell \
  target/wasm32-unknown-unknown/release/erax_web_shell.wasm
cp /tmp/wbg/erax-web-shell.js /tmp/wbg/erax-web-shell_bg.wasm ../web-assets/
```

`wasm-bindgen` must match the `wasm-bindgen` crate version
(`web-shell/Cargo.toml`). `web-shell` pins its own `[profile.release]`
(opt-level 2, LTO, single codegen unit, stripped): the default release
profile's parallel codegen units reorder functions nondeterministically
(two identical-source builds differed), while `codegen-units = 1` makes
the shipped wasm bit-reproducible (verified: 3 consecutive forced rebuilds
→ identical bytes). Sizes (release): wasm 4,828,725 B, JS glue 131,332 B,
`index.html` 1,640 B; raw `erax_web_shell.wasm` 6,452,429 B (debug: 97 MB —
never shipped). The wasm grew with the WebGL2 fallback backend (wgpu `webgl`
feature); the shipped bytes stay bit-reproducible.

**Security model (non-negotiable):**
- Opt-in only (`--web-server`). Binds `127.0.0.1` by default.
- Per-session random token REQUIRED (printed at startup, never stored,
  never reused). No token / wrong token → the connection is closed.
- Binding a non-loopback address requires the explicit acknowledging flag:
  `erax --web-server --web-bind 0.0.0.0:8080 --web-allow-remote`.
  Without `--web-allow-remote`, a non-loopback bind is refused.
- Custom bind: `erax --web-server --web-bind 127.0.0.1:9000`.

**Architecture:** Core+TUI run server-side (headless); the GUI presentation
core runs client-side in wasm (WebGPU canvas). The wire protocol is typed
and versioned (`src/web/protocol.rs`): dirty regions + N-273 semantic
metadata out, raw input (`KeyPress`/`MouseEvent`) in. The GUI stays a dumb
passthrough — all input models and mode-aware UI live in the backend.

**Browser-reserved keys** (`Ctrl+W/N/Tab`, …) go to the browser, never to the
editor — documented per input model in `KEYBINDINGS.md` ("WebUI notes").
They are never silently swallowed and never faked as parity.

## Configuration

Configuration is compiled into the binary via `src/user_config.rs`. There is no runtime configuration file. I've done my best to make this file easy to read and edit, I'll be working on making it even easier as I polish things up a bit.

## Building from Source (I haven't tested this with Make very much, cargo is recommended for the time being.)

```bash
git clone https://github.com/bonzupii/erax.git
cd erax
make # Read makefile or run "make help" to read about other build options.
sudo make install # Only if you want to install to your /usr/local/bin directory. otherwise just use ./target/release/erax to run the editor directly
```

Or use cargo directly:

```bash
cargo build --release 
./target/release/erax
```

## Known Limitations

- Some keybindings may not work properly, the ones that do may not work in all terminal emulators.
- Large file performance (>100MB) is largely untested at the current buffer implementation; an in-house B-tree data structure with a pager is planned (see `docs/DATA-STRUCTURE.md` if present in the working tree).
- Only tested on Fedora Linux, your mileage may vary on other distros or operating systems. Theoretically it should run on mac or windows but I have not tested it.
- Documentation is incomplete, innaccurate in many places, and not polished.
- The text engine is currently `ropey` (lossless UTF-8 core). Corrupted input is handled by the encoding layer (`src/core/encoding.rs`): detection, decode-with-replacement, and byte-exact round-trips for unmodified files. `ropey` is slated for replacement by the in-house data structure.
- Syntax highlighting could be smarter and more generalized.
- I've chosen many lightweight but experimental alternatives to heavy IDE features that one would expect in a code editor, these features will be refined and improved in future releases.
