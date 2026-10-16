# erax accessibility profiles

Preconfigured accessibility setups, shipped as `.patch` files against
the stock config. Enable with one command:

```sh
./accessibility.sh <profile>...
```

The script applies the named patches and runs `cargo build --release`
in one step. Windows: `accessibility.bat` does the same.

## Profiles

| Profile | Class | What it changes |
|---|---|---|
| `blind` | blindness | Enables the accessibility cue bus (`CUES_ENABLED`). The stock config already sets `ANNOUNCE_CURSOR_LINE` and `CUE_ERROR_BELL`, so the bus master is the only flip. |
| `deaf` | hearing | Enables the accessibility cue bus (`CUES_ENABLED`). The stock config already sets `CUE_VISUAL_FLASH`, the visual equivalent of the audio cues, so the bus master is the only flip. |
| `speech` | blindness | Selects the built-in speech backend (`SPEECH_BACKEND = BuiltIn`): Whisper large-v3-turbo q5_0 STT and Kokoro-82M int8 TTS. build.rs provisions the pinned weights (~690 MiB download, sha256-verified, embedded); `speech.flags` passes `--features speech` to the build. |

## Composition rules

Profiles compose **by class**: each profile touches disjoint config
ground, so application order does not matter. Disjointness is
structural, not trusted to `patch --forward`: the cue-bus master
(`CUES_ENABLED`) is derived from per-class profile constants
(`BLIND_PROFILE`, `DEAF_PROFILE`), so no two profiles ever patch the
same line. (An earlier design shared the `CUES_ENABLED` hunk between
profiles and relied on `--forward` to skip it; testing showed GNU
patch does not skip cleanly — it writes `.rej` files and exits
non-zero — so the shared hunk was removed.)

Applying an already-applied profile is a no-op; a patch that fits
neither forward nor reversed aborts the script loudly, which is how
config drift surfaces instead of silently misbuilding.

Profiles are **reversible**: `./accessibility.sh --unapply
<profile>...` reverse-applies in reverse order. To change the profile
set, reverse everything first, then apply the new set.

## Building the `speech` profile: network requirements

The `speech` profile needs two network fetches at build time, both
fail-loud:

1. **Model weights** (build.rs): the pinned Whisper, Kokoro, and
   voice-bank artifacts (~690 MiB), each size- and sha256-verified
   against the pins in `build/speech_artifacts.rs`, then embedded in
   the binary. A mismatch fails the build and removes the bad file.
2. **ONNX Runtime library** (`ort-sys` build script): a prebuilt
   `onnxruntime` binary downloaded from `cdn.pyke.io`. This fetch is
   outside erax's sha256 regime — it is the dependency's build
   script doing it, and erax's build.rs cannot pin it (dependency
   build scripts run before erax's own). If the CDN is unreachable,
   the speech build fails; the escape hatch is to point `ort-sys`
   at a local copy of the official onnxruntime 1.31.0 release:

   ```sh
   export ORT_LIB_PATH=/path/to/onnxruntime-1.31.0/lib
   export ORT_PREFER_DYNAMIC_LINK=1
   ./accessibility.sh speech
   ```

   (The second variable matters: without it `ort-sys` attempts a
   static link against the directory and fails; the official release
   ships only shared libraries.)

   Note the release build embeds ~750 MiB of weights, so it needs
   roughly 8 GB of RAM with LTO; `CARGO_PROFILE_RELEASE_LTO=off`
   lowers that if the linker gets killed.

## Profile honesty rule

A profile may only flip config constants that exist and are wired.
If a disability class has no wired settings yet, it gets no profile;
the missing profile is a roadmap signal, not a placeholder.
