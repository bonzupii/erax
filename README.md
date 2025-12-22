# EraX

A text editor under active development, written in Rust.

EraX is an experimental multi-modal text editor with GUI, TUI, and stream editing modes. It uses uEmacs-style (not emacs!!) keybindings and aims for a single static binary with minimal dependencies. It aims to be as self-contained as possible.

**Status**: Alpha (v0.8.0-alpha). Many features are incomplete or experimental. Risk of editor malfunction and data loss are high if you choose to use the editor in this stage.

## License

GNU General Public License, version 2 only (GPL-2.0-only).

## Features

- **Terminal mode (TUI)**: Interactive editing with `crossterm`. Supports ASCII, ANSI, and UTF-8 terminals. Also has GPM mouse support for the 3 people in the world who actually use GPM.
- **Graphical mode (GUI)**: GPU-accelerated rendering via `wgpu` and `winit`.
- **Stream mode (Sed)**: Basic POSIX-style stream editing for scripting. Also supports "diff-mode" for viewing and approving diffs for sed-style edit commands.
- **Syntax highlighting**: Rule-based lexer for C, Rust, Python, Go, JavaScript. (no heavy tree-sitter! The highlighting may not be "semantically correct" but it looks pretty and makes code more readable for the 99% of people who don't care about semantic correctness of syntax highlighting.)
- **uEmacs-style keybindings**: Common navigation and editing commands.
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
- Large file performance (>100MB) is largely untested, but for the few large tests I've done it seemed reasonably performant.
- Only tested on Fedora Linux, your mileage may vary on other distros or operating systems. Theoretically it should run on mac or windows but I have not tested it.
- Documentation is incomplete and not polished. The --help command line arguement was broken during a refactoring stage and has not yet been restored.
- Lossy UTF-8 decoding for corrupted/invalid UTF-8 has significant performance issues and is mostly untested at this time. Lossless UTF-8 encoding seems to work fine.
- Syntax highlighting could be smarter and more generalized.
- I've chosen many lightweight but experimental alternatives to heavy IDE features that one would expect in a code editor, these features will be refined and improved in future releases.
- Spellchecker exists but is largely untested in terms of accuracy and functionality. It is very fast and can eat garbage and call it a dictionary. We'll see how that goes.
