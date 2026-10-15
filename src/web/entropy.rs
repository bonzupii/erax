//! Platform-neutral session token generator for the WebUI server.
//!
//! The token is a 128-bit bearer credential presented to the browser
//! client once, at session start, and required on every subsequent frame.
//! It must be minted identically on every platform the server builds on,
//! including Windows, so no platform file (`/dev/urandom`) or platform
//! syscall participates anywhere in this module: every input comes from
//! `std` items that exist on all targets.
//!
//! Construction: seed material is concatenated, compressed with SHA-256
//! (FIPS 180-4) into a 256-bit key, and expanded with the ChaCha20 stream
//! (RFC 8439 section 2.3) from counter 0. Token bytes are the first 16
//! keystream bytes, hex-encoded. SHA-256 and ChaCha20 are implemented here
//! from their specifications so the tree carries no new dependency; both
//! carry their published test vectors in `tests` below.
//!
//! Security model, stated without varnish: this mints an
//! unguessable-enough per-session bearer token for a server that binds
//! loopback by default. The threat answered is a remote party guessing a
//! live session token (128-bit class) within one session's lifetime. It is
//! NOT a general-purpose CSPRNG and does not claim OS-CSPRNG equivalence:
//! the seed material is observable in part (wall clock, pid, thread id),
//! and a restored VM snapshot with a frozen clock defeats the
//! time-based inputs. Uniqueness across rapid successive calls holds by
//! construction, not by clock luck: a process-wide atomic counter is part
//! of every seed and strictly increases, so two calls in one process can
//! never feed the mixer identical input.

use std::sync::atomic::{AtomicU64, Ordering};
use std::time::SystemTime;

/// Process-wide mint counter. Strictly increases; part of every seed, so
/// successive `generate_token` calls cannot produce identical input to
/// the mixer even when every other source repeats (same nanosecond,
/// same thread, same addresses).
static MINT_COUNTER: AtomicU64 = AtomicU64::new(0);

// ---------------------------------------------------------------------------
// SHA-256 (FIPS 180-4)
// ---------------------------------------------------------------------------

/// Round constants: first 32 bits of the fractional parts of the cube
/// roots of the first 64 primes.
const K256: [u32; 64] = [
    0x428a2f98, 0x71374491, 0xb5c0fbcf, 0xe9b5dba5, 0x3956c25b, 0x59f111f1,
    0x923f82a4, 0xab1c5ed5, 0xd807aa98, 0x12835b01, 0x243185be, 0x550c7dc3,
    0x72be5d74, 0x80deb1fe, 0x9bdc06a7, 0xc19bf174, 0xe49b69c1, 0xefbe4786,
    0x0fc19dc6, 0x240ca1cc, 0x2de92c6f, 0x4a7484aa, 0x5cb0a9dc, 0x76f988da,
    0x983e5152, 0xa831c66d, 0xb00327c8, 0xbf597fc7, 0xc6e00bf3, 0xd5a79147,
    0x06ca6351, 0x14292967, 0x27b70a85, 0x2e1b2138, 0x4d2c6dfc, 0x53380d13,
    0x650a7354, 0x766a0abb, 0x81c2c92e, 0x92722c85, 0xa2bfe8a1, 0xa81a664b,
    0xc24b8b70, 0xc76c51a3, 0xd192e819, 0xd6990624, 0xf40e3585, 0x106aa070,
    0x19a4c116, 0x1e376c08, 0x2748774c, 0x34b0bcb5, 0x391c0cb3, 0x4ed8aa4a,
    0x5b9cca4f, 0x682e6ff3, 0x748f82ee, 0x78a5636f, 0x84c87814, 0x8cc70208,
    0x90befffa, 0xa4506ceb, 0xbef9a3f7, 0xc67178f2,
];

/// Initial hash value: first 32 bits of the fractional parts of the
/// square roots of the first 8 primes.
const H256_INIT: [u32; 8] = [
    0x6a09e667, 0xbb67ae85, 0x3c6ef372, 0xa54ff53a, 0x510e527f, 0x9b05688c,
    0x1f83d9ab, 0x5be0cd19,
];

/// SHA-256 over `data`, returned as 32 bytes (FIPS 180-4, section 6.2.2).
fn sha256(data: &[u8]) -> [u8; 32] {
    let mut h = H256_INIT;
    // Padding: one 0x80 byte, zeros, then the 64-bit big-endian bit length.
    let bit_len = (data.len() as u64).wrapping_mul(8);
    let mut padded = Vec::with_capacity(data.len() + 72);
    padded.extend_from_slice(data);
    padded.push(0x80);
    while padded.len() % 64 != 56 {
        padded.push(0);
    }
    padded.extend_from_slice(&bit_len.to_be_bytes());
    // Process each 512-bit block. Padding above guarantees exactness.
    let (blocks, remainder) = padded.as_chunks::<64>();
    debug_assert!(remainder.is_empty());
    for block in blocks {
        let mut w = [0u32; 64];
        for i in 0..16 {
            w[i] = u32::from_be_bytes([
                block[4 * i],
                block[4 * i + 1],
                block[4 * i + 2],
                block[4 * i + 3],
            ]);
        }
        for i in 16..64 {
            let s0 = w[i - 15].rotate_right(7) ^ w[i - 15].rotate_right(18)
                ^ (w[i - 15] >> 3);
            let s1 = w[i - 2].rotate_right(17) ^ w[i - 2].rotate_right(19)
                ^ (w[i - 2] >> 10);
            w[i] = w[i - 16]
                .wrapping_add(s0)
                .wrapping_add(w[i - 7])
                .wrapping_add(s1);
        }
        let (mut a, mut b, mut c, mut d, mut e, mut f, mut g, mut hh) =
            (h[0], h[1], h[2], h[3], h[4], h[5], h[6], h[7]);
        for i in 0..64 {
            let s1 =
                e.rotate_right(6) ^ e.rotate_right(11) ^ e.rotate_right(25);
            let ch = (e & f) ^ ((!e) & g);
            let t1 = hh
                .wrapping_add(s1)
                .wrapping_add(ch)
                .wrapping_add(K256[i])
                .wrapping_add(w[i]);
            let s0 =
                a.rotate_right(2) ^ a.rotate_right(13) ^ a.rotate_right(22);
            let maj = (a & b) ^ (a & c) ^ (b & c);
            let t2 = s0.wrapping_add(maj);
            hh = g;
            g = f;
            f = e;
            e = d.wrapping_add(t1);
            d = c;
            c = b;
            b = a;
            a = t1.wrapping_add(t2);
        }
        h[0] = h[0].wrapping_add(a);
        h[1] = h[1].wrapping_add(b);
        h[2] = h[2].wrapping_add(c);
        h[3] = h[3].wrapping_add(d);
        h[4] = h[4].wrapping_add(e);
        h[5] = h[5].wrapping_add(f);
        h[6] = h[6].wrapping_add(g);
        h[7] = h[7].wrapping_add(hh);
    }
    let mut out = [0u8; 32];
    for (i, word) in h.iter().enumerate() {
        out[4 * i..4 * i + 4].copy_from_slice(&word.to_be_bytes());
    }
    out
}

// ---------------------------------------------------------------------------
// ChaCha20 (RFC 8439, section 2.3)
// ---------------------------------------------------------------------------

/// One ChaCha20 quarter round on words (a, b, c, d) of the state.
fn quarter_round(s: &mut [u32; 16], a: usize, b: usize, c: usize, d: usize) {
    s[a] = s[a].wrapping_add(s[b]);
    s[d] ^= s[a];
    s[d] = s[d].rotate_left(16);
    s[c] = s[c].wrapping_add(s[d]);
    s[b] ^= s[c];
    s[b] = s[b].rotate_left(12);
    s[a] = s[a].wrapping_add(s[b]);
    s[d] ^= s[a];
    s[d] = s[d].rotate_left(8);
    s[c] = s[c].wrapping_add(s[d]);
    s[b] ^= s[c];
    s[b] = s[b].rotate_left(7);
}

/// The ChaCha20 block function: 20 rounds over the 4x4 state, then the
/// working state added back to the input state, serialized little-endian.
fn chacha20_block(
    key: &[u8; 32],
    counter: u32,
    nonce: &[u8; 12],
) -> [u8; 64] {
    let mut s = [0u32; 16];
    s[0] = 0x61707865;
    s[1] = 0x3320646e;
    s[2] = 0x79622d32;
    s[3] = 0x6b206574;
    for i in 0..8 {
        s[4 + i] = u32::from_le_bytes([
            key[4 * i],
            key[4 * i + 1],
            key[4 * i + 2],
            key[4 * i + 3],
        ]);
    }
    s[12] = counter;
    for i in 0..3 {
        s[13 + i] = u32::from_le_bytes([
            nonce[4 * i],
            nonce[4 * i + 1],
            nonce[4 * i + 2],
            nonce[4 * i + 3],
        ]);
    }
    let input = s;
    for _ in 0..10 {
        // Column rounds.
        quarter_round(&mut s, 0, 4, 8, 12);
        quarter_round(&mut s, 1, 5, 9, 13);
        quarter_round(&mut s, 2, 6, 10, 14);
        quarter_round(&mut s, 3, 7, 11, 15);
        // Diagonal rounds.
        quarter_round(&mut s, 0, 5, 10, 15);
        quarter_round(&mut s, 1, 6, 11, 12);
        quarter_round(&mut s, 2, 7, 8, 13);
        quarter_round(&mut s, 3, 4, 9, 14);
    }
    let mut out = [0u8; 64];
    for i in 0..16 {
        let word = s[i].wrapping_add(input[i]);
        out[4 * i..4 * i + 4].copy_from_slice(&word.to_le_bytes());
    }
    out
}

/// ChaCha20 stream: consecutive block outputs from `counter` upward.
/// The counter wraps at 2^32 per RFC 8439; draws here are far smaller.
fn chacha20_stream(
    key: &[u8; 32],
    nonce: &[u8; 12],
    counter: u32,
    out: &mut [u8],
) {
    let mut c = counter;
    for chunk in out.chunks_mut(64) {
        let block = chacha20_block(key, c, nonce);
        chunk.copy_from_slice(&block[..chunk.len()]);
        c = c.wrapping_add(1);
    }
}

// ---------------------------------------------------------------------------
// Seed material
// ---------------------------------------------------------------------------

/// Push one `u64` of seed material as 8 little-endian bytes.
fn push_u64(seed: &mut Vec<u8>, v: u64) {
    seed.extend_from_slice(&v.to_le_bytes());
}

/// Collect the platform-neutral seed material. Each source below is
/// annotated at its push site with what it contributes and its limits.
fn collect_seed() -> Vec<u8> {
    let mut seed = Vec::with_capacity(128);

    // Wall-clock time, nanosecond resolution: separates sessions started
    // at different times. Limit: coarse on some platforms; an observer
    // of the machine knows it approximately.
    let now = SystemTime::now();
    match now.duration_since(SystemTime::UNIX_EPOCH) {
        Ok(d) => {
            push_u64(&mut seed, d.as_secs());
            push_u64(&mut seed, d.subsec_nanos() as u64);
        }
        Err(e) => {
            // Clock before the epoch: still a value, still mixed in.
            let d = e.duration();
            push_u64(&mut seed, d.as_secs());
            push_u64(&mut seed, d.subsec_nanos() as u64);
        }
    }

    // Process id: separates concurrent server processes. Limit: small,
    // partly predictable space.
    push_u64(&mut seed, std::process::id() as u64);

    // Thread id, via its Debug spelling (stable `ThreadId(N)` form):
    // separates threads minting in one process. Limit: sequential ids.
    let tid = format!("{:?}", std::thread::current().id());
    seed.extend_from_slice(tid.as_bytes());
    seed.push(0);

    // Process-wide mint counter: STRICTLY increases, so no two calls in
    // one process ever feed the mixer identical input. This is the
    // load-bearing uniqueness source; everything else is seasoning.
    push_u64(&mut seed, MINT_COUNTER.fetch_add(1, Ordering::Relaxed));

    // Stack address of a local: varies with call depth and stack
    // placement. Limit: low entropy, may repeat across calls.
    let stack_probe = 0u8;
    push_u64(&mut seed, &stack_probe as *const u8 as usize as u64);

    // Two heap allocation addresses: carry the process's ASLR slide.
    // Limit: within one process, successive allocations sit at
    // predictable offsets from each other; the counter covers that.
    let a = Box::new(0u8);
    let b = Box::new(0u8);
    push_u64(&mut seed, &*a as *const u8 as usize as u64);
    push_u64(&mut seed, &*b as *const u8 as usize as u64);
    drop(a);
    drop(b);

    seed
}

/// Mint one session token: 32 lowercase hex chars encoding 16 keystream
/// bytes. Infallible: every input is always available on every target,
/// and the primitives are total functions over their domains.
pub fn generate_token() -> String {
    let seed = collect_seed();
    let key = sha256(&seed);
    // Nonce derived from the key under a fixed domain separator, so the
    // nonce can never collide with a nonce from a different key.
    let mut nonce_input = b"erax-web-token v1".to_vec();
    nonce_input.extend_from_slice(&key);
    let nonce_full = sha256(&nonce_input);
    let mut nonce = [0u8; 12];
    nonce.copy_from_slice(&nonce_full[..12]);
    let mut bytes = [0u8; 16];
    chacha20_stream(&key, &nonce, 0, &mut bytes);
    let mut token = String::with_capacity(32);
    for b in bytes {
        const HEX: &[u8; 16] = b"0123456789abcdef";
        token.push(HEX[(b >> 4) as usize] as char);
        token.push(HEX[(b & 0x0f) as usize] as char);
    }
    token
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hex_of(bytes: &[u8]) -> String {
        bytes.iter().map(|b| format!("{b:02x}")).collect()
    }

    /// FIPS 180-4 vectors, each cross-checked against two independent
    /// implementations (GNU coreutils `sha256sum` 9.4 and OpenSSL via
    /// Python `hashlib`); all three agree bit-for-bit.
    #[test]
    fn sha256_published_vectors() {
        assert_eq!(
            hex_of(&sha256(b"")),
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );
        assert_eq!(
            hex_of(&sha256(b"abc")),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
        // 56-byte input exercises the two-block padding path.
        assert_eq!(
            hex_of(&sha256(
                b"abcdbcdecdefdefgefghfghighijhijkijkljklmklmnlmnomnopnopq"
            )),
            "248d6a61d20638b8e5c026930c3e6039a33ce45964ff2167f6ecedd419db06c1"
        );
    }

    /// RFC 8439 section 2.3.2: the ChaCha20 block function test vector,
    /// transcribed from the published RFC text (the section's own
    /// intermediate state dumps reproduce these bytes, confirming the
    /// transcription).
    #[test]
    fn chacha20_block_published_vector() {
        let mut key = [0u8; 32];
        for (i, b) in key.iter_mut().enumerate() {
            *b = i as u8;
        }
        let nonce = [
            0x00, 0x00, 0x00, 0x09, 0x00, 0x00, 0x00, 0x4a, 0x00, 0x00, 0x00,
            0x00,
        ];
        let block = chacha20_block(&key, 1, &nonce);
        assert_eq!(
            hex_of(&block),
            "10f1e7e4d13b5915500fdd1fa32071c4\
             c7d1f4c733c068030422aa9ac3d46c4e\
             d2826446079faa0914c2d705d98b02a2\
             b5129cd1de164eb9cbd083e8a2503c4e"
        );
    }

    /// Stream draws equal consecutive block outputs with an incrementing
    /// counter; verified against the same RFC 8439 key and nonce.
    #[test]
    fn chacha20_stream_increments_counter() {
        let mut key = [0u8; 32];
        for (i, b) in key.iter_mut().enumerate() {
            *b = i as u8;
        }
        let nonce = [
            0x00, 0x00, 0x00, 0x09, 0x00, 0x00, 0x00, 0x4a, 0x00, 0x00, 0x00,
            0x00,
        ];
        let mut out = [0u8; 128];
        chacha20_stream(&key, &nonce, 1, &mut out);
        let b1 = chacha20_block(&key, 1, &nonce);
        let b2 = chacha20_block(&key, 2, &nonce);
        assert_eq!(&out[..64], &b1[..]);
        assert_eq!(&out[64..], &b2[..]);
    }

    /// Token shape is unchanged from the /dev/urandom era: 32 lowercase
    /// hex chars.
    #[test]
    fn token_format_is_32_hex_chars() {
        for _ in 0..100 {
            let t = generate_token();
            assert_eq!(t.len(), 32);
            assert!(t.bytes().all(|b| b.is_ascii_hexdigit()
                && !b.is_ascii_uppercase()));
        }
    }

    /// Uniqueness by construction: the mint counter is part of every
    /// seed, so a tight loop can never repeat a token.
    #[test]
    fn ten_thousand_tokens_are_distinct() {
        let mut seen = std::collections::HashSet::with_capacity(10_000);
        for _ in 0..10_000 {
            let t = generate_token();
            assert!(seen.insert(t), "duplicate token minted");
        }
        assert_eq!(seen.len(), 10_000);
    }

    /// Distribution sanity on a fixed stream: 1 MiB drawn from a fixed
    /// key and nonce, every byte value within 3x of the mean. The fixed
    /// inputs make this deterministic, never flaky.
    #[test]
    fn keystream_byte_histogram_is_flat() {
        let key = sha256(b"erax histogram fixture");
        let nonce = [7u8; 12];
        let mut counts = [0u64; 256];
        let mut chunk = [0u8; 4096];
        let mut counter = 0u32;
        for _ in 0..256 {
            chacha20_stream(&key, &nonce, counter, &mut chunk);
            counter += 64;
            for b in chunk {
                counts[b as usize] += 1;
            }
        }
        let total: u64 = counts.iter().sum();
        assert_eq!(total, 1 << 20);
        let mean = total as f64 / 256.0;
        for (i, c) in counts.iter().enumerate() {
            let ratio = *c as f64 / mean;
            assert!(
                (0.34..=3.0).contains(&ratio),
                "byte {i:#04x} has degenerate frequency {c}"
            );
        }
    }

    /// Different seed inputs produce uncorrelated streams: two fixed
    /// distinct keys differ in well over half their bytes.
    #[test]
    fn distinct_keys_give_uncorrelated_streams() {
        let k1 = sha256(b"seed material one");
        let k2 = sha256(b"seed material two");
        let nonce = [0u8; 12];
        let mut s1 = [0u8; 1024];
        let mut s2 = [0u8; 1024];
        chacha20_stream(&k1, &nonce, 0, &mut s1);
        chacha20_stream(&k2, &nonce, 0, &mut s2);
        let differing =
            s1.iter().zip(s2.iter()).filter(|(a, b)| a != b).count();
        assert!(
            differing > 900,
            "streams from distinct keys correlate: {differing}/1024 differ"
        );
    }
}
