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

## Composition rules

Profiles compose **by class**: each profile touches disjoint config
ground, so application order does not matter. The one shared line is
the `CUES_ENABLED` master in `src/config/accessibility/mod.rs`; the
script applies patches with `patch --forward`, which skips a hunk
already applied by another profile, so any order converges to the
same tree.

Profiles are **reversible**: `./accessibility.sh --unapply
<profile>...` reverse-applies in reverse order. To change the profile
set, reverse everything first, then apply the new set.

## Profile honesty rule

A profile may only flip config constants that exist and are wired.
If a disability class has no wired settings yet, it gets no profile;
the missing profile is a roadmap signal, not a placeholder.
