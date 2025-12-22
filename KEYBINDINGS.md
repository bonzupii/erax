# erax Keybindings

This document serves as the authoritative reference for keybindings in erax. The editor preserves classic command names and default sequences for seamless workflow migration.

## Global Prefix Keys

- `^X`: Control-X (Prefix Key)
- `ESC-`: Meta/Alt (Meta Prefix)
- `^U`: Universal Argument (Numeric Prefix)

## Movement Commands

| Key Sequence | Command | Description |
|--------------|---------|-------------|
| `^F` / `Right` | `forward-character` | Move forward one character |
| `^B` / `Left` | `backward-character` | Move backward one character |
| `^N` / `Down` | `next-line` | Move to next line |
| `^P` / `Up` | `previous-line` | Move to previous line |
| `^A` / `Home` | `beginning-of-line` | Move to start of line |
| `^E` / `End` | `end-of-line` | Move to end of line |
| `^V` / `PgDn` | `forward-page` | Scroll down one page |
| `ESC-V` / `PgUp` | `backward-page` | Scroll up one page |
| `ESC-<` | `beginning-of-file` | Move to start of buffer |
| `ESC->` | `end-of-file` | Move to end of buffer |
| `ESC-f` | `forward-word` | Move forward one word |
| `ESC-b` | `backward-word` | Move backward one word |
| `ESC-g` | `goto-line` | Jump to specific line |
| `ESC-{` | `backward-paragraph` | Move to previous paragraph |
| `ESC-}` | `forward-paragraph` | Move to next paragraph |

## Editing Commands

| Key Sequence | Command | Description |
|--------------|---------|-------------|
| `^D` / `Del` | `delete-char` | Delete character under cursor |
| `Backspace` | `delete-backward-char` | Delete character before cursor |
| `^K` | `kill-to-end-of-line` | Kill text to end of line |
| `^Y` | `yank` | Insert last killed text |
| `^W` | `kill-region` | Kill text between mark and cursor |
| `ESC-w` | `copy-region` | Copy text between mark and cursor |
| `^O` | `open-line` | Insert newline and move up |
| `^T` | `transpose-characters` | Swap two characters |
| `ESC-t` | `transpose-words` | Swap two words |
| `ESC-d` | `kill-word` | Kill word forward |
| `ESC-BSP` | `backward-kill-word` | Kill word backward |
| `ESC-q` | `justify-paragraph` | Reflow current paragraph |
| `^X^O` | `delete-blank-lines` | Delete blank lines around point |

## File & Buffer Operations

| Key Sequence | Command | Description |
|--------------|---------|-------------|
| `^X^F` | `find-file` | Open or create a file |
| `^X^S` | `save-buffer` | Save current buffer |
| `^X^W` | `write-file` | Save buffer as new name |
| `^X^R` | `read-file` | Insert file at cursor |
| `^X B` | `select-buffer` | Switch to named buffer |
| `^X K` | `delete-buffer` | Close current buffer |
| `^X^C` | `exit-erax` | Quit editor |

## Window Management

| Key Sequence | Command | Description |
|--------------|---------|-------------|
| `^X 2` | `split-current-window` | Split window vertically |
| `^X 3` | `split-window-horizontally`| Split window horizontally |
| `^X 0` | `minimize-window` | Close current window |
| `^X 1` | `delete-other-windows` | Expand current window to full screen |
| `^X o` | `next-window` | Move focus to next window |
| `^X Z` | `grow-window` | Increase window height |
| `^X ^Z` | `shrink-window` | Decrease window height |

## Search & Replace

| Key Sequence | Command | Description |
|--------------|---------|-------------|
| `^S` | `search-forward` | Search forward incrementally |
| `^R` | `search-reverse` | Search backward incrementally |
| `ESC-%` | `query-replace` | Interactive find and replace |

## Advanced Commands

| Key Sequence | Command | Description |
|--------------|---------|-------------|
| `^X (` | `begin-macro` | Start recording macro |
| `^X )` | `end-macro` | Stop recording macro |
| `^X e` | `execute-macro` | Execute last macro |
| `^X #` | `calculator` | Bitwise programmer's calculator |
| `ESC-^F` | `goto-matching-fence` | Jump to matching bracket |
| `^X !` | `shell-command` | Execute external shell command |
| `ESC-x` | `execute-named-command` | Run command by name (M-x) |

## Modern Extensions

| Key Sequence | Command | Description |
|--------------|---------|-------------|
| `ESC-/` | `word-completion` | Buffer-local word completion |
| `^X d` | `toggle-diagnostics` | Show/hide diagnostics pane |
| `^X t 2` | `split-spawn-terminal-v`| Spawn terminal in vertical split |
| `^X t 3` | `split-spawn-terminal-h`| Spawn terminal in horizontal split |