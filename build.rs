//! Build-time speech weight provisioning.
//!
//! Reads the speech backend selection from
//! `src/config/accessibility/blindness.rs` with an anchored pattern
//! that fails loudly on config drift. When the backend is `BuiltIn`,
//! downloads the pinned weight artifacts, verifies size and sha256
//! into `OUT_DIR`, and emits the cfg/env the speech modules embed
//! with `include_bytes!`. When the backend is `BuiltIn` but the
//! `speech` cargo feature is off, emits `cfg(speech_misconfigured)`
//! so the crate root fails with a precise `compile_error!`. A
//! download or hash failure fails the build loudly; there is no
//! silent degradation to an unprovisioned build. Nothing is written
//! outside `OUT_DIR`, and nothing runs at editor runtime.

use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

include!("build/speech_artifacts.rs");

fn main() {
    println!("cargo:rerun-if-changed=src/config/accessibility/blindness.rs");
    println!("cargo:rustc-check-cfg=cfg(speech_provisioned)");
    println!("cargo:rustc-check-cfg=cfg(speech_misconfigured)");

    match read_speech_backend().as_str() {
        "Off" | "Custom" => return,
        "BuiltIn" => {}
        other => panic!("build.rs: unknown SPEECH_BACKEND variant: {other}"),
    }
    if env::var("CARGO_FEATURE_SPEECH").is_err() {
        println!("cargo:rustc-cfg=speech_misconfigured");
        return;
    }
    let out = PathBuf::from(env::var("OUT_DIR").expect("build.rs: OUT_DIR not set"))
        .join("speech-weights");
    for (name, url, sha256, size) in ARTIFACTS {
        provision(&out, name, url, sha256, *size);
    }
    println!("cargo:rustc-cfg=speech_provisioned");
    println!("cargo:rustc-env=ERAX_SPEECH_WEIGHTS_DIR={}", out.display());
}

/// Read the `SPEECH_BACKEND` variant with an anchored match on the
/// exact const declaration. Any drift in the config shape fails the
/// build instead of silently provisioning (or skipping) weights.
fn read_speech_backend() -> String {
    let src = fs::read_to_string("src/config/accessibility/blindness.rs")
        .expect("build.rs: cannot read src/config/accessibility/blindness.rs");
    for line in src.lines() {
        if let Some(rest) = line
            .trim()
            .strip_prefix("pub const SPEECH_BACKEND: SpeechBackend = SpeechBackend::")
        {
            return rest.trim_end_matches(';').trim().to_string();
        }
    }
    panic!(
        "build.rs: SPEECH_BACKEND declaration not found in \
         src/config/accessibility/blindness.rs (config drift?)"
    );
}

fn provision(out: &Path, name: &str, url: &str, sha256: &str, size: u64) {
    let dest = out.join(name);
    if dest.exists() {
        let fresh = fs::metadata(&dest)
            .map(|meta| meta.len() == size && hash_file(&dest) == sha256)
            .unwrap_or(false);
        if fresh {
            return;
        }
        fs::remove_file(&dest).expect("build.rs: cannot remove stale weight file");
    }
    if let Some(parent) = dest.parent() {
        fs::create_dir_all(parent).expect("build.rs: cannot create weights dir");
    }
    download(url, &dest);
    let len = fs::metadata(&dest)
        .expect("build.rs: downloaded file vanished")
        .len();
    if len != size {
        fs::remove_file(&dest).ok();
        panic!("build.rs: size mismatch for {name}: got {len}, want {size}");
    }
    if hash_file(&dest) != sha256 {
        fs::remove_file(&dest).ok();
        panic!("build.rs: sha256 mismatch for {name} (removed the bad file)");
    }
}

fn download(url: &str, dest: &Path) {
    let tmp = dest.with_extension("part");
    let status = Command::new("curl")
        .args([
            "--fail",
            "--silent",
            "--show-error",
            "--location",
            "--retry",
            "3",
            "-o",
        ])
        .arg(&tmp)
        .arg(url)
        .status()
        .expect("build.rs: curl not found; install curl to provision speech weights");
    if !status.success() {
        panic!("build.rs: download failed: {url}");
    }
    fs::rename(&tmp, dest).expect("build.rs: cannot move downloaded file into place");
}

fn hash_file(path: &Path) -> String {
    use sha2::{Digest, Sha256};
    let bytes = fs::read(path).expect("build.rs: cannot read downloaded file");
    format!("{:x}", Sha256::digest(bytes))
}
