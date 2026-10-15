<!-- GENERATED from src/config/keys.rs (the compile-time typed keymaps). Do not hand-edit the tables; regenerate with the documented procedure. -->

# erax Keybindings

Input model is a compile-time `pub const` (`InputModel`, `src/config/keys.rs`);
invalid configuration is a compile error. Default model: KeyTrie (Emacs/uemacs).

Command names are the typed `Command` enum variants rendered kebab-case.

## Model 1 — KeyTrie (Emacs/uemacs, default)

Classic uemacs chords and prefix sequences. Unlisted printable keys insert themselves.

| Keys | Command |
|------|---------|
| `C-f` | `forward-character` |
| `C-b` | `backward-character` |
| `C-n` | `next-line` |
| `C-p` | `previous-line` |
| `Right` | `forward-character` |
| `Left` | `backward-character` |
| `Down` | `next-line` |
| `Up` | `previous-line` |
| `Home` | `beginning-of-line` |
| `End` | `end-of-line` |
| `PgUp` | `backward-page` |
| `PgDn` | `forward-page` |
| `Ins` | `toggle-overwrite-mode` |
| `C-a` | `beginning-of-line` |
| `C-e` | `end-of-line` |
| `C-v` | `forward-page` |
| `M-V` | `backward-page` |
| `M-v` | `backward-page` |
| `M-<` | `beginning-of-file` |
| `M->` | `end-of-file` |
| `M-f` | `forward-word` |
| `M-b` | `backward-word` |
| `M-t` | `transpose-words` |
| `M-x` | `execute-named-command` |
| `M-g` | `goto-line` |
| `M-}` | `forward-paragraph` |
| `M-{` | `backward-paragraph` |
| `C-M-f` | `goto-matching-fence` |
| `C-x` `C-f` | `find-file` |
| `C-x` `C-s` | `save-buffer` |
| `C-x` `C-w` | `write-file` |
| `C-x` `C-c` | `exit-erax` |
| `C-x` `C-r` | `read-file` |
| `ESC` `P` | `print` |
| `C-d` | `delete-next-character` |
| `DEL` | `delete-next-character` |
| `BS` | `delete-previous-character` |
| `RET` | `insert-newline` |
| `TAB` | `insert-tab` |
| `C-c` `TAB` | `expand-snippet` |
| `C-k` | `kill-to-end-of-line` |
| `C-y` | `yank` |
| `M-y` | `yank-pop` |
| `C-w` | `kill-region` |
| `M-w` | `copy-region` |
| `C-o` | `open-line` |
| `C-t` | `transpose-characters` |
| `M-/` | `word-completion` |
| `C-x` `TAB` | `word-completion` |
| `C-s` | `search-forward` |
| `C-r` | `search-reverse` |
| `M-%` | `query-replace` |
| `C-x` `2` | `split-current-window` |
| `C-x` `3` | `split-window-horizontally` |
| `C-x` `0` | `minimize-window` |
| `C-x` `9` | `window-picker` |
| `C-x` `1` | `delete-other-windows` |
| `C-x` `o` | `next-window` |
| `C-x` `O` | `next-window` |
| `C-x` `Z` | `grow-window` |
| `C-x` `C-z` | `shrink-window` |
| `C-z` | `backward-page` |
| `C-c` | `insert-space` |
| `C-j` | `newline-and-indent` |
| `C-l` | `redraw-display` |
| `C-q` | `quote-character` |
| `C-u` | `universal-argument` |
| `C-g` | `keyboard-quit` |
| `C-x` `B` | `select-buffer` |
| `C-x` `K` | `delete-buffer` |
| `C-x` `d` | `toggle-diagnostics` |
| `C-x` `=` | `show-position` |
| `C-x` `?` | `describe-key` |
| `M-j` | `justify-paragraph` |
| `M-q` | `justify-paragraph` |
| `M-z` | `exit-and-save` |
| `M-u` | `case-word-upper` |
| `M-l` | `case-word-lower` |
| `M-c` | `case-word-capitalize` |
| `C-x` `C-u` | `uppercase-region` |
| `C-x` `C-l` | `lowercase-region` |
| `C-x` `4` | `shell-command` |
| `C-x` `p` | `calculator` |
| `C-x` `t` `2` | `split-spawn-terminal-vertical` |
| `C-x` `t` `3` | `split-spawn-terminal-horizontal` |
| `C-x` `C-o` | `delete-blank-lines` |
| `M-s` | `sed-preview` |
| `M-=` | `count-words` |
| `C-@` | `set-mark` |
| `M-.` | `set-mark` |
| `C-x` `C-x` | `exchange-point-and-mark` |
| `C-=` | `expand-selection` |
| `M-@` | `mark-word` |
| `M-h` | `mark-paragraph` |
| `M-DEL` | `kill-word` |
| `M-BS` | `backward-kill-word` |
| `C-x` `(` | `begin-macro` |
| `C-x` `)` | `end-macro` |
| `C-x` `e` | `execute-macro` |
| `C-x` `u` | `undo` |
| `C-_` | `undo` |
| `C-x` `U` | `redo` |
| `M-n` | `diagnostics-next` |
| `M-p` | `diagnostics-previous` |
| `F10` | `toggle-menu` |

## Model 2 — Modal (Vim)

Real modal editing: Normal / Insert / Visual / Operator-pending / Command-line.
Counts compose (`3dw`, `d3w`); operators `d`/`y`/`c` take motions; `"` selects registers; `.` repeats.

### Normal mode

| Key | Action |
|-----|--------|
| `h` | `backward-character` |
| `j` | `next-line` |
| `k` | `previous-line` |
| `l` | `forward-character` |
| `w` | word forward (`w`) |
| `b` | word backward (`b`) |
| `0` | `beginning-of-line` |
| `$` | `end-of-line` |
| `^` | `smart-home` |
| `g` | `g` prefix |
| `G` | `end-of-file` |
| `{` | `backward-paragraph` |
| `}` | `forward-paragraph` |
| `%` | `goto-matching-fence` |
| `f` | find char forward (`f`) |
| `F` | find char backward (`F`) |
| `t` | till char forward (`t`) |
| `T` | till char backward (`T`) |
| `;` | repeat find forward (`;`) |
| `,` | repeat find backward (`,`) |
| `d` | operator: delete |
| `y` | operator: yank |
| `c` | operator: change |
| `x` | delete char under cursor (`x`) |
| `X` | delete char before cursor (`X`) |
| `r` | replace char (`r`) |
| `s` | delete char + enter Insert (`s`) |
| `S` | delete line + enter Insert (`S`) |
| `D` | delete to end of line (`D`) |
| `C` | change to end of line (`C`) |
| `p` | put after cursor (`p`) |
| `P` | put before cursor (`P`) |
| `u` | `undo` |
| `"` | select register (`"x`) |
| `.` | repeat last change (`.`) |
| `/` | `search-forward` |
| `:` | enter CommandLine mode |
| `v` | enter Visual mode |
| `V` | enter Visual-Line mode (`V`) |
| `i` | enter Insert mode |
| `I` | insert at first non-blank (`I`) |
| `a` | append after cursor (`a`) |
| `A` | append at end of line (`A`) |
| `o` | open line below (`o`) |
| `O` | open line above (`O`) |

### Insert mode

| Key | Action |
|-----|--------|
| `ESC` | enter Normal mode |

### Visual mode

| Key | Action |
|-----|--------|
| `h` | `backward-character` |
| `j` | `next-line` |
| `k` | `previous-line` |
| `l` | `forward-character` |
| `w` | word forward (`w`) |
| `b` | word backward (`b`) |
| `0` | `beginning-of-line` |
| `$` | `end-of-line` |
| `G` | `end-of-file` |
| `f` | find char forward (`f`) |
| `F` | find char backward (`F`) |
| `t` | till char forward (`t`) |
| `T` | till char backward (`T`) |
| `d` | `kill-region` |
| `x` | `kill-region` |
| `y` | `copy-region` |
| `c` | `kill-region` |
| `v` | enter Normal mode |
| `ESC` | enter Normal mode |

### CommandLine mode

| Key | Action |
|-----|--------|

(`:` enters CommandLine; `/` starts search; `v`/`V` enter Visual; `i`/`a`/`o` etc. enter Insert; `Esc` returns to Normal.)

## Model 3 — Standard (Notepad / VS Code feel)

Modeless: typing inserts, CUA clipboard, selection-first editing.

| Keys | Command |
|------|---------|
| `C-c` | `copy-region` |
| `C-x` | `kill-region` |
| `C-v` | `yank` |
| `C-s` | `save-buffer` |
| `C-o` | `find-file` |
| `C-n` | `new-buffer` |
| `C-w` | `delete-buffer` |
| `C-z` | `undo` |
| `C-y` | `redo` |
| `C-S-z` | `redo` |
| `C-f` | `search-forward` |
| `C-h` | `query-replace` |
| `C-a` | `mark-whole-buffer` |
| `C-TAB` | `next-buffer` |
| `C-S-TAB` | `previous-buffer` |
| `Right` | `forward-character` |
| `Left` | `backward-character` |
| `Down` | `next-line` |
| `Up` | `previous-line` |
| `Home` | `smart-home` |
| `End` | `end-of-line` |
| `PgUp` | `backward-page` |
| `PgDn` | `forward-page` |
| `C-Left` | `backward-word` |
| `C-Right` | `forward-word` |
| `C-Home` | `beginning-of-file` |
| `C-End` | `end-of-file` |
| `S-Right` | `extend-selection-right` |
| `S-Left` | `extend-selection-left` |
| `S-Down` | `extend-selection-down` |
| `S-Up` | `extend-selection-up` |
| `S-Home` | `extend-selection-home` |
| `S-End` | `extend-selection-end` |
| `BS` | `delete-previous-character` |
| `DEL` | `delete-next-character` |
| `RET` | `insert-newline` |
| `TAB` | `insert-tab` |
| `Ins` | `toggle-overwrite-mode` |
| `F10` | `toggle-menu` |

## WebUI notes

Keys the browser reserves cannot be captured by the web shell. The web shell
never calls `preventDefault()` on them and never fakes parity: they go to the
browser, and the editor never sees them. This is per input model:

**Browser-reserved (all models):** the page does NOT preventDefault, so these
always go to the browser:
- `Ctrl+W` — close tab (browser). Not available in WebUI in any model.
- `Ctrl+N` — new window (browser). Not available in WebUI in any model.
- `Ctrl+T` — new tab (browser). Not available in WebUI in any model.
- `Ctrl+Tab` / `Ctrl+Shift+Tab` — tab switching (browser). Not available.
- `Alt+Left` / `Alt+Right` — history navigation (browser). Not available.

**Model 1 — KeyTrie (Emacs):** no default Emacs chords collide with the
reserved set above. `Ctrl+W` is not a default Emacs binding in erax's KeyTrie
model, so nothing is lost.

**Model 2 — Modal (Vim):** Vim's `Ctrl+W` (window commands) is unavailable in
WebUI — the browser closes the tab instead. Window management in WebUI must
use the `:` command line or menu. This is a documented limitation, not a
silent remap.

**Model 3 — Standard (CUA):** `Ctrl+W` (close window/tab in CUA convention)
is unavailable — the browser owns it. Use the browser tab close UI.

No binding is silently swallowed: if the browser takes a key, the editor does
nothing (the key never reaches the backend). The web shell's
`is_browser_reserved()` (`src/gui/shell/web.rs`) is the allowlist; anything
not on it is forwarded with `preventDefault()` so the editor gets it.

## Regenerating this document

The tables above are generated, not hand-written. To regenerate after changing
`src/config/keys.rs`:

1. Create `examples/dump_keymaps.rs` (temporary, not committed) that imports
   `erax::config::keys::{KEYBINDINGS, MODAL_BINDINGS, STANDARD_BINDINGS}`,
   `erax::core::input::KeyPress`, and prints Markdown rows using
   `KeyPress`'s `Display` (`C-`/`M-`/`S-` prefixes) and `Command::name()`
   (kebab-case). Modal actions are rendered per `ModalAction` variant.
2. Run `cargo run --example dump_keymaps > /tmp/keymaps.md`, prepend the
   GENERATED comment header, and write the result to `KEYBINDINGS.md`.
3. Delete the example (`rm -rf examples/`).

This keeps the document byte-accurate with the compiled keymaps (L-003:
the old hand-maintained tables documented commands deleted in N-205 and
contradicted the binary on `^X^C`).
