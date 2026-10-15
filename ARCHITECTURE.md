# erax Architecture

This document describes the internal design and system architecture of the erax editor.

## Design Philosophy

erax follows a "Data-View-Controller" separation where the core engine is headless and agnostic of the frontend (Terminal or GUI).

1. **Safety**: Zero `unsafe` code in the core editing engine.
2. **Performance**: O(log n) buffer operations via B-tree ropes.
3. **Robustness**: ID-based handle system to avoid pointer invalidation and satisfy Rust's borrow checker.

## Core Components

### 1. ID Handle System (`src/core/id.rs`)

To manage complex relationships between buffers and windows without resorting to reference counting or unsafe pointers, erax uses a unique ID system:

- `BufferId`: A unique index into the global buffer map.
- `WindowId`: A unique index into the global window map.

### 2. Buffer Management (`src/core/buffer.rs`)

Buffers represent the raw text data. They are decoupled from any view state.

- **Storage**: Currently backed by `ropey::Rope`. An in-house B-tree data
  structure (piece-style leaves, pager-first file access) is designed and
  pending implementation; it will replace `ropey` behind the same `Buffer`
  API.
- **Encodings** (`src/core/encoding.rs`): BOM detection, UTF-16/32, ISO-8859 /
  Windows code pages; decode-with-replacement for corrupted input and
  byte-exact round-trips for unmodified files. The tree itself stays
  encoding-dumb (bytes are bytes).
- **Streaming Load**: Uses `Rope::from_reader` with `BufReader` to stream large files directly into the rope structure, minimizing peak memory usage.
- **Undo/Redo**: Grouped edit operations with smart heuristic grouping (e.g., grouping character insertions but breaking on whitespace).
- **Diagnostics**: Container for compiler/lint errors associated with the buffer.
- **Syntax State Cache**: Each buffer maintains a `syntax_state_cache` vector tracking lexer state at line boundaries, enabling efficient syntax highlighting for multi-line constructs (e.g., block comments).

### 3. Window View State (`src/core/window.rs`)

Windows represent a viewport into a buffer. Multiple windows can view the same `BufferId`.

- **Cursor**: Tracked as `(x, y)` coordinates in grapheme clusters.
- **Scroll**: Tracks the top-most visible line (`scroll_offset`).
- **Visual Mapping**: Logic for converting byte offsets to visual column positions, accounting for multi-byte UTF-8 characters and tab widths.

### 4. Layout Engine (`src/core/layout.rs`)

The layout is managed as a binary tree of `LayoutNode`s.

- **Leaf Nodes**: Contain a single `WindowId`.
- **Internal Nodes**: Contain a split (Vertical or Horizontal) and two child nodes.
- **Recursive Rendering**: The frontend traverses this tree to calculate window geometries (`Rect`).

### 5. Command Dispatcher (`src/core/dispatcher.rs`)

All user actions are typed `Command` enum variants (kebab-case names via
`Command::name()`), dispatched by a `match` — no stringly registry, no
`HashMap<String, Box<dyn Command>>`.

- **Compile-time input models** (`src/config/keys.rs`): `InputModel::{KeyTrie, Modal, Standard}`
  selects one of three typed keymaps (`KEYBINDINGS`, `MODAL_BINDINGS`, `STANDARD_BINDINGS`).
  Invalid configuration is a compile error. `KEYBINDINGS.md` is generated from these consts.
- **Atomic Execution**: Commands receive a mutable reference to the `EditorApp` and a `count` parameter (the "universal argument").
- **Frontend Independence**: Commands manipulate state; the frontend is responsible for triggering a redraw based on the `DispatchResult`.

### 6. Screen Model and Mode-Aware UI (`src/terminal/display.rs`)

All mode UI state originates in the backend and reaches renderers only through
the TUI screen model plus N-273 semantic metadata. The GUI is a dumb
passthrough: it renders the screen model and forwards raw input; it owns no
editor state, logic, or input handling. The WebUI (planned) is a child of the
GUI, inheriting the same presentation core.

Per model the screen model exposes: current input state, prompt-bar
content/kind (`BarOwner` arbitration: prompt > command line > message > mode
line), pending key-sequence display, cursor style (typed `CursorStyle` const
mapping emitted as DECSCUSR to the terminal and as metadata for GUI shaders),
and selection kind. Emacs echo-area behavior is unchanged; Standard shows
`Ln X, Col Y (Z selected)`; Modal shows persistent `-- NORMAL --` etc.
segments with pending count+operator and an owning `:` command line.

## Data Flow

1. **Input**: Frontend (TUI/GUI) captures raw events.
2. **Translation**: `KeyBindingManager` maps event sequences to command names.
3. **Dispatch**: The dispatcher executes the command against the `EditorApp`.
4. **Synchronization**:
   - `Buffer` updates its version and marks itself dirty.
   - `Window` updates its cursor/scroll state.
5. **Render**: Frontend observes dirty flags and redraws modified components.

## Frontend Implementations

### Terminal (TUI)
Uses `crossterm` for raw mode and terminal control. Implements an incremental diff-based rendering engine in `src/terminal/display.rs` with `DirtyTracker` to minimize bandwidth over slow connections (SSH).

### Graphical (GUI)
Uses `wgpu` for high-performance glyph rendering. It maintains a GPU-resident texture atlas of glyphs. The GUI mode operates as a terminal emulator that renders the TUI's `ScreenBuffer` with sub-pixel precision.

### Sed
A specialized execution loop that bypasses the `Window` and `Layout` systems, applying `Buffer` operations directly to a stream.

## Threading Model

erax is currently single-threaded in its core to avoid synchronization overhead. PTY hosting for terminal emulation (`TerminalHost`) and background I/O for file loading utilize threads but communicate via message passing (`mpsc`).
