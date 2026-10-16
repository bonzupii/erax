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
/// provisioning anchor) does not rebuild the wasm. The fingerprint also
/// folds in the toolchain and generator versions, so neither can change
/// under the outputs without a rebuild.
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
    // The generator version pin lives in the root Cargo.lock, not in the
    // input set; anchor the lock file so a generator-only bump reruns
    // the script, and let the fingerprint (which folds the pin in)
    // decide whether the nested build fires.
    println!(
        "cargo:rerun-if-changed={}",
        manifest.join("Cargo.lock").display()
    );
    let out = PathBuf::from(env::var("OUT_DIR").expect("build.rs: OUT_DIR not set")).join(OUT_SUBDIR);
    println!("cargo:rustc-env=ERAX_WEB_ASSETS_OUT={}", out.display());
    if fingerprint_matches(&out, &inputs, &manifest) {
        return;
    }
    build_wasm(&shell, &out);
    run_bindgen(&out);
    write_fingerprint(&out, &inputs, &manifest);
}

/// Every file the wasm build reads: web-shell's own sources, the
/// #[path]-included files from the root crate (followed recursively to a
/// fixpoint), the include_str!/include_bytes! assets those sources bake
/// in (e.g. the WGSL shaders), and the web-shell manifests.
fn collect_inputs(shell: &Path) -> Vec<PathBuf> {
    let mut inputs = Vec::new();
    for name in ["Cargo.toml", "Cargo.lock"] {
        let p = shell.join(name);
        assert!(p.is_file(), "build.rs: missing {}", p.display());
        inputs.push(canonical(&p));
    }
    // Fixpoint over #[path] includes: a discovered target may itself
    // include further files (theme/themes.rs reaches the wasm only
    // through such a nested include).
    let mut seen = std::collections::HashSet::new();
    let mut work = Vec::new();
    let mut stack = vec![shell.join("src")];
    while let Some(dir) = stack.pop() {
        let entries = fs::read_dir(&dir).expect("build.rs: cannot read web-shell src");
        for entry in entries {
            let p = entry.expect("build.rs: bad dir entry").path();
            if p.is_dir() {
                stack.push(p);
            } else if p.extension().is_some_and(|e| e == "rs") {
                work.push(canonical(&p));
            }
        }
    }
    while let Some(f) = work.pop() {
        if !seen.insert(f.clone()) {
            continue;
        }
        inputs.push(f.clone());
        for inc in path_includes(&f) {
            if inc.extension().is_some_and(|e| e == "rs") {
                work.push(inc);
            } else {
                inputs.push(inc);
            }
        }
        inputs.extend(literal_includes(&f));
    }
    inputs.sort();
    inputs.dedup();
    inputs
}

/// The #[path = "..."] includes in one source file, resolved against the
/// file's own directory. A single-line heuristic: the line must open an
/// attribute (#[...]) and carry a path = "..." pair, which also covers
/// #[cfg_attr(..., path = "...")]. Multi-line attributes are not
/// recognized; the tree currently uses only the single-line form.
fn path_includes(file: &Path) -> Vec<PathBuf> {
    let src = fs::read_to_string(file).expect("build.rs: cannot read web-shell source");
    let dir = file.parent().expect("build.rs: source has no parent");
    let mut out = Vec::new();
    for line in src.lines() {
        let line = line.trim();
        if !line.starts_with("#[") {
            continue;
        }
        let Some(ppos) = line.find("path") else {
            continue;
        };
        let after = &line[ppos + "path".len()..];
        let Some(eq) = after.find('=') else {
            continue;
        };
        let quoted = after[eq + 1..].trim_start();
        let Some(rest) = quoted.strip_prefix('"') else {
            continue;
        };
        let Some(end) = rest.find('"') else {
            continue;
        };
        let rel = &rest[..end];
        if rel.is_empty() {
            continue;
        }
        out.push(canonical(&dir.join(rel)));
    }
    out
}

/// The include_str!/include_bytes! asset paths in one source file,
/// resolved against the file's own directory. Same single-line
/// heuristic as path_includes: the literal must sit on the macro's line.
fn literal_includes(file: &Path) -> Vec<PathBuf> {
    let src = fs::read_to_string(file).expect("build.rs: cannot read web-shell source");
    let dir = file.parent().expect("build.rs: source has no parent");
    let mut out = Vec::new();
    for line in src.lines() {
        for mac in ["include_str!", "include_bytes!"] {
            let Some(mpos) = line.find(mac) else {
                continue;
            };
            let after = &line[mpos + mac.len()..];
            let Some(open) = after.find('(') else {
                continue;
            };
            let quoted = after[open + 1..].trim_start();
            let Some(rest) = quoted.strip_prefix('"') else {
                continue;
            };
            let Some(end) = rest.find('"') else {
                continue;
            };
            let rel = &rest[..end];
            if rel.is_empty() {
                continue;
            }
            out.push(canonical(&dir.join(rel)));
        }
    }
    out
}

/// True when the fingerprint matches AND the embedded outputs exist.
fn fingerprint_matches(out: &Path, inputs: &[PathBuf], manifest: &Path) -> bool {
    let fp = out.join(FINGERPRINT_FILE);
    let Ok(saved) = fs::read_to_string(&fp) else {
        return false;
    };
    if saved.trim() != fingerprint(inputs, manifest) {
        return false;
    }
    EMBEDDED.iter().all(|n| out.join(n).is_file())
}

fn write_fingerprint(out: &Path, inputs: &[PathBuf], manifest: &Path) {
    fs::create_dir_all(out).expect("build.rs: cannot create web-assets out dir");
    fs::write(out.join(FINGERPRINT_FILE), fingerprint(inputs, manifest))
        .expect("build.rs: cannot write fingerprint");
}

/// sha256 over (path, length, mtime) of every input, sorted, plus the
/// toolchain and generator versions: neither is a file in the input set,
/// but both change what the nested build produces.
fn fingerprint(inputs: &[PathBuf], manifest: &Path) -> String {
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
    h.update(rustc_version().as_bytes());
    h.update([0]);
    h.update(bindgen_support_version(manifest).as_bytes());
    format!("{:x}", h.finalize())
}

/// The rustc the outer build runs under; a toolchain bump must rebuild
/// the wasm rather than re-embed a client built by the old one.
fn rustc_version() -> String {
    let rustc = env::var("RUSTC").unwrap_or_else(|_| "rustc".into());
    String::from_utf8_lossy(
        &Command::new(&rustc)
            .arg("--version")
            .output()
            .expect("build.rs: cannot run rustc --version")
            .stdout,
    )
    .trim()
    .to_string()
}

/// The pinned wasm-bindgen-cli-support version from the root Cargo.lock;
/// a generator-only bump must rebuild rather than re-embed the old glue.
fn bindgen_support_version(manifest: &Path) -> String {
    let lock =
        fs::read_to_string(manifest.join("Cargo.lock")).expect("build.rs: cannot read Cargo.lock");
    let mut in_pkg = false;
    for line in lock.lines() {
        let line = line.trim();
        if line == "[[package]]" {
            in_pkg = false;
            continue;
        }
        if line == r#"name = "wasm-bindgen-cli-support""# {
            in_pkg = true;
            continue;
        }
        if in_pkg && line.starts_with("version = ") {
            return line.to_string();
        }
    }
    panic!("build.rs: wasm-bindgen-cli-support not found in Cargo.lock");
}

/// Nested `cargo build --release --target wasm32-unknown-unknown` for the
/// web-shell crate, offline from the registry cache, with its own target
/// dir under OUT_DIR. A failure fails the build loudly: a server binary
/// without its client is a broken artifact, never a silent fallback.
fn build_wasm(shell: &Path, out: &Path) {
    let target_dir = out.join("wasm-target");
    // The nested cargo keys its build-script outputs on package,
    // features, target, and toolchain, not on the environment this
    // invocation supplies, so a cache written under a different
    // supplied environment is replayed verbatim. The stamp records
    // the environment contract of the invocation; a cache stamped
    // with any other contract is discarded and rebuilt.
    const ENV_CONTRACT: &str = "scrub-cargo-cfg-feature-1";
    let stamp = target_dir.join(".env-contract");
    let stamped = fs::read_to_string(&stamp).unwrap_or_default();
    if stamped != ENV_CONTRACT {
        if target_dir.exists() {
            fs::remove_dir_all(&target_dir).expect("build.rs: failed to discard stale wasm target dir");
        }
        fs::create_dir_all(&target_dir).expect("build.rs: failed to create wasm target dir");
        fs::write(&stamp, ENV_CONTRACT).expect("build.rs: failed to stamp wasm target dir");
    }
    let cargo = env::var("CARGO").expect("build.rs: CARGO not set");
    let mut cmd = Command::new(&cargo);
    cmd.args(["build", "--release", "--target", "wasm32-unknown-unknown", "--offline"])
        .current_dir(shell)
        .env("CARGO_TARGET_DIR", &target_dir)
        // The nested build must see the profile web-shell documents
        // ([profile.release] in its own Cargo.toml), not the outer
        // build's ambient flags.
        .env_remove("RUSTFLAGS")
        .env_remove("CARGO_ENCODED_RUSTFLAGS")
        // Compiler wrapper variables from the outer cargo invocation
        // (RUSTC_WORKSPACE_WRAPPER under `cargo clippy`, RUSTC_WRAPPER
        // generally) must not cross into the nested build: they
        // redirect rustc to the outer invocation's driver (e.g.
        // clippy-driver), which then fails the wasm32 target on lints
        // the plain build never sees.
        .env_remove("RUSTC_WORKSPACE_WRAPPER")
        .env_remove("RUSTC_WRAPPER");
    // Target and feature variables set by the outer cargo for this
    // build script (CARGO_CFG_*, CARGO_FEATURE_*) describe the outer
    // target and this crate's features. Dependency build scripts in
    // the nested build read them to evaluate cfg expressions, so any
    // left in the environment are applied to wasm32 units as if they
    // described the wasm32 target. The nested cargo sets the correct
    // values for its own build scripts; the inherited ones are
    // removed so none crosses over.
    for (key, _) in env::vars_os() {
        let name = key.to_string_lossy();
        if name.starts_with("CARGO_CFG_") || name.starts_with("CARGO_FEATURE_") {
            cmd.env_remove(&key);
        }
    }
    let status = cmd
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
