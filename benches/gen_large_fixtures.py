#!/usr/bin/env python3
"""Deterministic large-file fixture generator for the erax DS large-file program.

Recipes (all seeded; re-running reproduces byte-identical files):
  - lines_1m.txt   : 1,000,000 lines, ~80 MB
  - bytes_100m.txt : exactly 100 MiB (line-aligned)
  - bytes_1g.txt   : exactly 1 GiB  (line-aligned)
  - sparse_16g.bin : 16 GiB sparse file (> typical RAM); a few committed pages
                     at known offsets so mapped reads see real content.

Line mix: code-like ASCII lines, some with multi-byte UTF-8, tabs, and
long lines, so line indexing is representative. Seed: 0xERA5 (245661).
"""
import os
import random
import sys

SEED = 245661
# Outside the repo: fixtures are large binaries and must NOT ride in patch.diff.
OUT = os.path.expanduser("~/workspace/erax-work/fixtures")
os.makedirs(OUT, exist_ok=True)

TEMPLATE_BITS = [
    "fn function_{i}(arg: usize) -> usize {{ let result = arg * {i} + compute(i); result }}",
    "let binding_{i} = vec![{a}, {b}, {c}]; // trailing comment {i}",
    "\tfor k in 0..{i} {{ acc += table[k & 255] as usize; }}",
    "struct Record{i} {{ id: u64, name: String, 日本語: bool, active: bool }}",
    "// ── separator line {i} ─────────────────────────────────────────────",
    "if value_{i} > threshold && flag_{j} {{ emit!(\"event-{i}-{j} → done\"); }}",
    "longline_{i}_" + "y" * 40,
    "😀 emoji line {i} with multibyte → arrows and café",
]


def make_line(rng, i):
    t = TEMPLATE_BITS[rng.randrange(len(TEMPLATE_BITS))]
    a = rng.randrange(1000)
    b = rng.randrange(1000)
    c = rng.randrange(1000)
    j = rng.randrange(1000)
    return t.format(i=i, a=a, b=b, c=c, j=j) + "\n"


def gen_lines(path, n_lines):
    rng = random.Random(SEED)
    with open(path, "w", encoding="utf-8", newline="\n") as f:
        for i in range(n_lines):
            f.write(make_line(rng, i))
    print(f"wrote {path}: {n_lines} lines, {os.path.getsize(path)} bytes")


def gen_sized(path, size_bytes):
    rng = random.Random(SEED)
    written = 0
    i = 0
    with open(path, "w", encoding="utf-8", newline="\n") as f:
        while True:
            line = make_line(rng, i)
            enc = line.encode("utf-8")
            if written + len(enc) > size_bytes:
                break
            f.write(line)
            written += len(enc)
            i += 1
    # Pad the tail with spaces (no newline) to hit the exact size.
    with open(path, "ab") as f:
        f.write(b" " * (size_bytes - written))
    assert os.path.getsize(path) == size_bytes, os.path.getsize(path)
    print(f"wrote {path}: {i} lines + pad, exactly {size_bytes} bytes")


def gen_sparse(path, size_bytes):
    rng = random.Random(SEED)
    with open(path, "wb") as f:
        f.truncate(size_bytes)
        # Commit a page of content at a few known offsets: start, 1/4, 1/2,
        # 3/4, and near the end.
        for frac in (0.0, 0.25, 0.5, 0.75, 0.999):
            off = int(size_bytes * frac)
            f.seek(off)
            chunk = "".join(make_line(rng, k) for k in range(200)).encode("utf-8")
            f.write(chunk)
    # Sparse: apparent size is size_bytes, disk use is ~5 pages.
    st = os.stat(path)
    print(f"wrote {path}: apparent {st.st_size} bytes, blocks {st.st_blocks} "
          f"(~{st.st_blocks * 512 // 1024} KiB on disk)")


def main():
    which = sys.argv[1:] or ["all"]
    if "all" in which or "1m" in which:
        gen_lines(os.path.join(OUT, "lines_1m.txt"), 1_000_000)
    if "all" in which or "100m" in which:
        gen_sized(os.path.join(OUT, "bytes_100m.txt"), 100 * 1024 * 1024)
    if "all" in which or "1g" in which:
        gen_sized(os.path.join(OUT, "bytes_1g.txt"), 1024 * 1024 * 1024)
    if "all" in which or "sparse" in which:
        gen_sparse(os.path.join(OUT, "sparse_16g.bin"), 16 * 1024 * 1024 * 1024)


if __name__ == "__main__":
    main()
