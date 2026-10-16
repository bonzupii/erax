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

## Profile honesty rule

A profile may only flip config constants that exist and are wired.
If a disability class has no wired settings yet, it gets no profile;
the missing profile is a roadmap signal, not a placeholder.
