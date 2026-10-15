#!/usr/bin/env python3
"""Regenerate tests/fixtures/vim_corpus.json by driving real vim.

Each case is (buffer, key sequence). Vim runs the keys in normal mode via
stdin, then dumps the buffer lines, cursor position, and unnamed register
to a sidecar file. The JSON records vim's observable outcome; the Rust
test `core::modal::tests::vim_differential_corpus` replays the same keys
through erax's modal engine and asserts identical outcomes.

Requires: vim (9.x). Regenerate with:
    python3 tests/gen_vim_corpus.py
Run from the repo root. Slow (~2s per vim invocation); runs cases
sequentially for determinism.
"""
import json
import os
import subprocess
import sys
import tempfile

ESC = "\x1b"
CR = "\r"
CTRL_R = "\x12"

# Key tokens: "<Esc>", "<CR>", "<C-R>" are special; anything else is one char.
def to_vim_bytes(keys):
    out = []
    for k in keys:
        if k == "<Esc>":
            out.append(ESC)
        elif k == "<CR>":
            out.append(CR)
        elif k == "<C-R>":
            out.append(CTRL_R)
        else:
            assert len(k) == 1, k
            out.append(k)
    return "".join(out)

# Buffers (all newline-terminated; ASCII so byte col == char col).
B1 = "hello world\n"
B2 = "one two three four\n"
B3 = "  leading spaces\n"
B4 = "trailing spaces   \n"
B5 = "a  b   c\n"
B6 = "foo,bar;baz:qux\n"
B7 = "hello\n\nworld\n"
B8 = "\n"
B9 = "single\n"
B10 = "a\n"
B11 = "line one\nline two\nline three\n"
B12 = "(foo [bar] {baz})\n"
B13 = "((nested (deep)) outer)\n"
B14 = '"quoted" and \'single\'\n'
B15 = "word. Next sentence! Third?\nMore here.\n"
B16 = "   \n"
B18 = "snake_case word-here and CAPS\n"
B19 = "x\n"

# (name, buffer, keys)
CASES = [
    # -- word motions --
    ("w_basic", B1, ["w"]),
    ("w_2", B2, ["2", "w"]),
    ("w_3", B2, ["3", "w"]),
    ("w_at_end", B9, ["w"]),
    ("w_punct", B6, ["w"]),
    ("w_punct_2", B6, ["2", "w"]),
    ("b_basic", B1, ["w", "b"]),
    ("b_at_start", B1, ["b"]),
    ("b_2", B2, ["2", "w", "2", "b"]),
    ("e_basic", B1, ["e"]),
    ("e_2", B2, ["2", "e"]),
    ("e_at_end", B9, ["e"]),
    ("e_last_char", B1, ["4", "l", "e"]),
    ("e_punct", B6, ["e"]),
    ("ge_basic", B1, ["$", "g", "e"]),
    ("ge_at_start", B1, ["g", "e"]),
    ("ge_2", B2, ["$", "2", "g", "e"]),
    ("W_basic", B18, ["W"]),
    ("W_2", B2, ["2", "W"]),
    ("B_basic", B2, ["2", "W", "B"]),
    ("E_basic", B18, ["E"]),
    ("E_2", B2, ["2", "E"]),
    ("gE_basic", B2, ["$", "g", "E"]),
    ("w_leading_ws", B3, ["w"]),
    ("b_leading_ws", B3, ["$", "b"]),
    ("e_trailing_ws", B4, ["e"]),
    ("w_multi_ws", B5, ["w"]),
    ("e_multi_ws", B5, ["e"]),
    # -- sentence motions --
    ("rparen", B15, [")"]),
    ("rparen_2", B15, ["2", ")"]),
    ("lparen", B15, [")", "("]),
    ("lparen_at_start", B15, ["("]),
    ("rparen_at_end", B15, ["G", ")"]),
    # -- find char --
    ("f_char", B1, ["f", "o"]),
    ("f_repeat", B1, ["f", "o", ";"]),
    ("f_rev", B1, ["f", "o", ","]),
    ("F_char", B1, ["$", "F", "o"]),
    ("t_char", B1, ["t", "o"]),
    ("T_char", B1, ["$", "T", "o"]),
    ("f_missing", B1, ["f", "z"]),
    # -- line motions --
    ("dollar", B1, ["$"]),
    ("zero", B1, ["$", "0"]),
    ("caret", B3, ["$","^"]),
    ("G_end", B11, ["G"]),
    ("gg_start", B11, ["G", "g", "g"]),
    ("paragraph_fwd", B7, ["}"]),
    ("paragraph_bwd", B7, ["G", "{"]),
    ("match_paren", B12, ["%"]),
    ("match_bracket", B12, ["w", "%"]),
    # -- operators x motions --
    ("dw", B1, ["d", "w"]),
    ("dw_2", B2, ["2", "d", "w"]),
    ("d2w", B2, ["d", "2", "w"]),
    ("de", B1, ["d", "e"]),
    ("de_2", B2, ["d", "2", "e"]),
    ("db", B1, ["w", "d", "b"]),
    ("dge", B1, ["$", "d", "g", "e"]),
    ("dW", B18, ["d", "W"]),
    ("dE", B18, ["d", "E"]),
    ("dollar_op", B1, ["d", "$"]),
    ("d0", B1, ["$", "d", "0"]),
    ("df", B1, ["d", "f", "o"]),
    ("dt", B1, ["d", "t", "o"]),
    ("dF", B1, ["$", "d", "F", "l"]),
    ("d_paren", B15, ["d", ")"]),
    ("d_lparen", B15, [")", "d", "("]),
    ("yw_p", B1, ["y", "w", "w", "p"]),
    ("ye_p", B1, ["y", "e", "p"]),
    ("y2w_p", B2, ["y", "2", "w", "p"]),
    # -- change (cw special cases) --
    ("cw_word", B1, ["c", "w", "X", "<Esc>"]),
    ("cW_word", B1, ["c", "W", "X", "<Esc>"]),
    ("cw_ws", B3, ["c", "w", "X", "<Esc>"]),
    ("cw_punct", B6, ["2", "l", "c", "w", "X", "<Esc>"]),
    ("c2w", B2, ["c", "2", "w", "X", "<Esc>"]),
    ("c2e", B2, ["c", "2", "e", "X", "<Esc>"]),
    ("2cw", B2, ["2", "c", "w", "X", "<Esc>"]),
    ("ce", B1, ["c", "e", "X", "<Esc>"]),
    ("cE", B18, ["c", "E", "X", "<Esc>"]),
    # -- linewise --
    ("dd", B11, ["d", "d"]),
    ("dd_2", B11, ["2", "d", "d"]),
    ("yy_p", B11, ["y", "y", "p"]),
    ("cc", B1, ["c", "c", "X", "<Esc>"]),
    ("D", B1, ["w", "D"]),
    ("C", B1, ["w", "C", "X", "<Esc>"]),
    ("x", B1, ["x"]),
    ("x_3", B1, ["3", "x"]),
    ("X", B1, ["$", "X"]),
    ("s", B1, ["s", "X", "<Esc>"]),
    ("S", B11, ["j", "S", "X", "<Esc>"]),
    # -- text objects --
    ("diw", B1, ["d", "i", "w"]),
    ("daw", B1, ["d", "a", "w"]),
    ("diw_mid", B2, ["w", "d", "i", "w"]),
    ("daw_eol", B4, ["$", "d", "a", "w"]),
    ("d2aw", B2, ["d", "2", "a", "w"]),
    ("diW", B18, ["d", "i", "W"]),
    ("daW", B18, ["d", "a", "W"]),
    ("diw_ws", B5, ["l", "d", "i", "w"]),
    ("daw_ws", B5, ["l", "d", "a", "w"]),
    ("ciw", B1, ["c", "i", "w", "X", "<Esc>"]),
    ("yiw_p", B1, ["y", "i", "w", "$", "p"]),
    ("dipar", B12, ["d", "i", "("]),
    ("dapar", B12, ["d", "a", "("]),
    ("dibracket", B12, ["w", "d", "i", "["]),
    ("dabracket", B12, ["w", "d", "a", "["]),
    ("dibrace", B12, ["2", "w", "d", "i", "{"]),
    ("dabrace", B12, ["2", "w", "d", "a", "{"]),
    ("dinested", B13, ["4", "l", "d", "i", "("]),
    ("d2inested", B13, ["4", "l", "d", "2", "i", "("]),
    ("danested_outer", B13, ["d", "a", "("]),
    ("dipar_between", B12, ["$", "d", "i", "("]),
    ("dipar_none", B1, ["d", "i", "("]),
    ("diquote", B14, ["d", "i", '"']),
    ("daquote", B14, ["d", "a", '"']),
    ("disquote", B14, ["w", "d", "i", "'"]),
    ("dasquote", B14, ["w", "d", "a", "'"]),
    ("diquote_none", B1, ["d", "i", '"']),
    ("diquote_on_open", B14, ["d", "i", '"']),
    # -- dot repeat --
    ("dot_dw", B2, ["d", "w", "."]),
    ("dot_diw", B2, ["w", "d", "i", "w", "."]),
    ("dot_cw", B2, ["c", "w", "X", "<Esc>", "w", "."]),
    # -- registers --
    ("named_yank_put", B2, ['"', "a", "y", "w", "w", '"', "a", "p"]),
    ("named_delete", B1, ['"', "b", "d", "w"]),
    ("named_overwrite_keeps_unnamed", B2, ['"', "a", "y", "w", "y", "w", '"', "a", "p"]),
    # -- undo/redo --
    ("undo", B1, ["d", "w", "u"]),
    ("undo_2", B2, ["d", "w", "d", "w", "2", "u"]),
    ("redo", B1, ["d", "w", "u", "<C-R>"]),
    ("redo_2", B2, ["d", "w", "d", "w", "u", "u", "<C-R>", "<C-R>"]),
    ("redo_count", B2, ["d", "w", "d", "w", "u", "u", "2", "<C-R>"]),
    # -- visual --
    ("visual_w_d", B1, ["v", "w", "d"]),
    ("visual_e_y", B1, ["v", "e", "y"]),
    ("visual_iw", B1, ["v", "i", "w", "d"]),
    ("visual_line", B11, ["V", "j", "d"]),
    ("visual_paren", B15, ["v", ")", "d"]),
    # -- insert --
    ("insert_type", B1, ["i", "X", "Y", "<Esc>"]),
    ("append", B1, ["a", "X", "<Esc>"]),
    ("open_below", B11, ["o", "X", "<Esc>"]),
    # -- edge positions --
    ("w_empty_line", B7, ["j", "w"]),
    ("dw_empty_line", B7, ["j", "d", "w"]),
    ("e_single_char", B10, ["e"]),
    ("de_single_char", B10, ["d", "e"]),
    ("x_single_char", B19, ["x"]),
    ("dw_ws_only", B16, ["d", "w"]),
    ("diw_ws_only", B16, ["d", "i", "w"]),
    ("b_multiline", B11, ["G", "b"]),
    ("w_multiline", B11, ["w"]),
    ("e_multiline", B11, ["2", "e"]),
]

def capture_cmd(out_path):
    return (
        ESC
        + ":call writefile(getline(1,'$') + [\"\\x1fCURSOR\\x1f\", "
        + "line('.').':'.col('.'), substitute(@\", '\\n', '<NL>', 'g')], '"
        + out_path.replace("'", "''")
        + "')\n:q!\n"
    )

_SENTINEL = "\x1fCURSOR\x1f"

def run_case(tmpdir, name, buffer, keys):
    case_txt = os.path.join(tmpdir, "case.txt")
    out_path = os.path.join(tmpdir, "case.out")
    with open(case_txt, "w") as f:
        f.write(buffer)
    key_bytes = to_vim_bytes(keys) + capture_cmd(out_path)
    try:
        subprocess.run(
            # --clean (not -u NONE): bare -u NONE starts the cursor on the
            # first non-blank instead of 1:1 in this vim build.
            ["vim", "--clean", "-n", "-i", "NONE", case_txt],
            input=key_bytes.encode(),
            stdout=subprocess.DEVNULL,
            stderr=subprocess.DEVNULL,
            timeout=15,
            check=False,
        )
    except subprocess.TimeoutExpired:
        return {"name": name, "error": "vim timeout"}
    if not os.path.exists(out_path):
        return {"name": name, "error": "no output from vim"}
    with open(out_path, "rb") as f:
        raw = f.read().decode("utf-8", errors="replace")
    os.remove(out_path)
    lines = raw.split("\n")
    # [... buffer lines ..., SENTINEL, "l:c", escaped-register, ""]
    try:
        si = lines.index(_SENTINEL)
    except ValueError:
        return {"name": name, "error": "no sentinel in %r" % raw[:80]}
    text_lines = lines[:si]
    rest = lines[si + 1 :]
    if len(rest) < 2:
        return {"name": name, "error": "short tail: %r" % raw[:80]}
    cursor_s, reg_escaped = rest[0], rest[1]
    try:
        ln, col = cursor_s.split(":")
        cursor = [int(ln) - 1, int(col) - 1]
    except ValueError:
        return {"name": name, "error": "bad cursor: %r" % cursor_s}
    text = "\n".join(text_lines) + "\n"
    reg = reg_escaped.replace("<NL>", "\n")
    return {
        "name": name,
        "buffer": buffer,
        "keys": keys,
        "vim_text": text,
        "vim_cursor": cursor,
        "vim_register": reg,
    }

def main():
    repo_root = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
    os.chdir(repo_root)
    for name, buffer, keys in CASES:
        assert _SENTINEL not in buffer, name
        assert "<NL>" not in buffer, name
    out = {"generated_by": "tests/gen_vim_corpus.py (real vim oracle)", "cases": []}
    errors = []
    with tempfile.TemporaryDirectory() as tmpdir:
        for name, buffer, keys in CASES:
            res = run_case(tmpdir, name, buffer, keys)
            if "error" in res:
                errors.append((name, res["error"]))
            out["cases"].append(res)
    fixture = "tests/fixtures/vim_corpus.json"
    os.makedirs(os.path.dirname(fixture), exist_ok=True)
    with open(fixture, "w") as f:
        json.dump(out, f, indent=1, sort_keys=True)
        f.write("\n")
    print("wrote %s (%d cases)" % (fixture, len(out["cases"])))
    if errors:
        print("ERRORS:")
        for name, err in errors:
            print("  %s: %s" % (name, err))
        sys.exit(1)

if __name__ == "__main__":
    main()
