/// Build-time generation of the web client (wasm + JS glue).
///
/// The web-shell crate is compiled for wasm32-unknown-unknown by a nested
/// cargo invocation (its own target dir under OUT_DIR, so the outer build's
/// locks are never contended) and run through wasm-bindgen's generation
/// library — the same transform the wasm-bindgen CLI performs. The
/// generated files land in OUT_DIR/web-assets; the server embeds them via
/// include_bytes! over the ERAX_WEB_ASSETS_OUT env emitted here.
///
/// Cost control: every input file is anchored with cargo:rerun-if-changed,
/// so cargo only reruns the build script when an input changed. The
/// expensive nested build carries its own fingerprint over the input set
/// on top, so an unrelated build-script trigger (e.g. the speech
/// provisioning anchor) does not rebuild the wasm.
///
/// (std::{env, fs, path, process} come from build.rs's own imports; this
/// file is include!d into its scope like build/speech_artifacts.rs, so
/// its items are documented with /// — //! would be a compile error
/// pasted mid-file.)
use sha2::{Digest, Sha256};
use wasm_bindgen_cli_support::Bindgen;

/// Subdirectory of OUT_DIR holding the generated client.
const OUT_SUBDIR: &str = "web-assets";

/// Fingerprint file recording the input set the outputs were built from.
const FINGERPRINT_FILE: &str = ".inputs-fingerprint";

/// The two files the server embeds; their absence forces a rebuild.
const EMBEDDED: [&str; 2] = ["erax-web-shell.js", "erax-web-shell_bg.wasm"];

/// Generate the client if the inputs changed. Always emits the
/// ERAX_WEB_ASSETS_OUT env the server embeds from.
pub fn web_assets_generate() {
    let manifest =
        PathBuf::from(env::var("CARGO_MANIFEST_DIR").expect("build.rs: CARGO_MANIFEST_DIR not set"));
    let shell = manifest.join("web-shell");
    let inputs = collect_inputs(&shell);
    for p in &inputs {
        println!("cargo:rerun-if-changed={}", p.display());
    }
    let out = PathBuf::from(env::var("OUT_DIR").expect("build.rs: OUT_DIR not set")).join(OUT_SUBDIR);
    println!("cargo:rustc-env=ERAX_WEB_ASSETS_OUT={}", out.display());
    if fingerprint_matches(&out, &inputs) {
        return;
    }
    build_wasm(&shell, &out);
    run_bindgen(&out);
    write_fingerprint(&out, &inputs);
}

/// Every file the wasm build reads: web-shell's own sources, the
/// #[path]-included files from the root crate, and the manifests.
fn collect_inputs(shell: &Path) -> Vec<PathBuf> {
    let mut inputs = Vec::new();
    for name in ["Cargo.toml", "Cargo.lock"] {
        let p = shell.join(name);
        assert!(p.is_file(), "build.rs: missing {}", p.display());
        inputs.push(canonical(&p));
    }
    let mut stack = vec![shell.join("src")];
    while let Some(dir) = stack.pop() {
        let entries = fs::read_dir(&dir).expect("build.rs: cannot read web-shell src");
        for entry in entries {
            let p = entry.expect("build.rs: bad dir entry").path();
            if p.is_dir() {
                stack.push(p);
            } else if p.extension().is_some_and(|e| e == "rs") {
                let canon = canonical(&p);
                inputs.push(canon.clone());
                inputs.extend(path_includes(&canon));
            }
        }
    }
    inputs.sort();
    inputs.dedup();
    inputs
}

/// The #[path = "..."] includes in one web-shell source file, resolved
/// against the file's own directory. This is the exact inclusion set
/// the compiler sees; enumerating it here keeps the anchor list from
/// drifting when includes are added or moved.
fn path_includes(file: &Path) -> Vec<PathBuf> {
    let src = fs::read_to_string(file).expect("build.rs: cannot read web-shell source");
    let dir = file.parent().expect("build.rs: source has no parent");
    let mut out = Vec::new();
    for line in src.lines() {
        let line = line.trim();
        let Some(rest) = line.strip_prefix("#[path") else {
            continue;
        };
        let Some(eq) = rest.find('=') else {
            continue;
        };
        let quoted = rest[eq + 1..].trim().trim_end_matches(']');
        let rel = quoted.trim_matches('"');
        if rel.is_empty() {
            continue;
        }
        out.push(canonical(&dir.join(rel)));
    }
    out
}

/// True when the fingerprint matches AND the embedded outputs exist.
fn fingerprint_matches(out: &Path, inputs: &[PathBuf]) -> bool {
    let fp = out.join(FINGERPRINT_FILE);
    let Ok(saved) = fs::read_to_string(&fp) else {
        return false;
    };
    if saved.trim() != fingerprint(inputs) {
        return false;
    }
    EMBEDDED.iter().all(|n| out.join(n).is_file())
}

fn write_fingerprint(out: &Path, inputs: &[PathBuf]) {
    fs::create_dir_all(out).expect("build.rs: cannot create web-assets out dir");
    fs::write(out.join(FINGERPRINT_FILE), fingerprint(inputs)).expect("build.rs: cannot write fingerprint");
}

/// sha256 over (path, length, mtime) of every input, sorted.
fn fingerprint(inputs: &[PathBuf]) -> String {
    let mut h = Sha256::new();
    for p in inputs {
        let meta = fs::metadata(p).expect("build.rs: cannot stat web input");
        let mtime = meta
            .modified()
            .expect("build.rs: no mtime")
            .duration_since(std::time::UNIX_EPOCH)
            .expect("build.rs: mtime before epoch")
            .as_nanos();
        h.update(p.to_string_lossy().as_bytes());
        h.update([0]);
        h.update(meta.len().to_le_bytes());
        h.update(mtime.to_le_bytes());
    }
    format!("{:x}", h.finalize())
}

/// Nested `cargo build --release --target wasm32-unknown-unknown` for the
/// web-shell crate, offline from the registry cache, with its own target
/// dir under OUT_DIR. A failure fails the build loudly: a server binary
/// without its client is a broken artifact, never a silent fallback.
fn build_wasm(shell: &Path, out: &Path) {
    let target_dir = out.join("wasm-target");
    let cargo = env::var("CARGO").expect("build.rs: CARGO not set");
    let status = Command::new(&cargo)
        .args(["build", "--release", "--target", "wasm32-unknown-unknown", "--offline"])
        .current_dir(shell)
        .env("CARGO_TARGET_DIR", &target_dir)
        .status()
        .expect("build.rs: failed to run nested cargo for the web shell");
    if !status.success() {
        panic!("build.rs: web-shell wasm build failed");
    }
    let wasm = target_dir.join("wasm32-unknown-unknown/release/erax_web_shell.wasm");
    assert!(wasm.is_file(), "build.rs: expected {}", wasm.display());
}

/// Run the built wasm through wasm-bindgen's library (the --target web
/// transform), emitting erax-web-shell.js / erax-web-shell_bg.wasm (plus
/// the TypeScript declarations, the CLI's default) into the out dir.
fn run_bindgen(out: &Path) {
    let wasm = out.join("wasm-target/wasm32-unknown-unknown/release/erax_web_shell.wasm");
    let mut b = Bindgen::new();
    b.input_path(&wasm);
    b.web(true)
        .unwrap_or_else(|e| panic!("build.rs: wasm-bindgen --target web: {e:?}"));
    b.out_name("erax-web-shell");
    // The page calls init() with no arguments, so the glue must carry the
    // default wasm path (new URL('erax-web-shell_bg.wasm', import.meta.url)).
    // The library defaults omit_default_module_path to true; the CLI
    // (whose output this replaces) defaults it to false.
    b.omit_default_module_path(false);
    b.typescript(true);
    b.generate(out)
        .unwrap_or_else(|e| panic!("build.rs: wasm-bindgen generate: {e:?}"));
}

fn canonical(p: &Path) -> PathBuf {
    fs::canonicalize(p).unwrap_or_else(|_| panic!("build.rs: cannot resolve {}", p.display()))
}
