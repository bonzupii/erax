# erax — Nix configuration layer

erax is configured at **compile time**: settings are typed `pub const`s in
`src/config/*.rs`, and an invalid configuration is a compile error, not a
runtime one. That design makes the usual "install a generic binary, drop a
config file next to it" flow a poor fit — but it makes Nix a *perfect* fit.
Nix is also compile-time and declarative; this directory layers the two.

## Pieces

- `flake.nix` — exposes `nixosModules.default` and the `lib` patch macros.
  Drop this flake into your `/etc/nixos` flake inputs.
- `module.nix` — `programs.erax`: `enable`, `src`, and typed `settings`
  (theme, input model, font size, plus an `extraOverrides` escape hatch
  for any other const).
- `lib.nix` — the patch macros: each setting becomes an anchored
  substitution against one `pub const` declaration line in `src/config/`,
  applied in `postPatch` before cargo builds. If an anchor const is
  renamed or removed upstream, the build **fails loudly** instead of
  silently shipping an unconfigured editor.

## Usage

```nix
# /etc/nixos/flake.nix
{
  inputs.erax-config.url = "path:/path/to/erax/nix/config";
  outputs = { nixpkgs, erax-config, ... }: {
    nixosConfigurations.myhost = nixpkgs.lib.nixosSystem {
      modules = [
        erax-config.nixosModules.default
        {
          programs.erax = {
            enable = true;
            src = /path/to/erax;  # your checkout or a pinned input
            settings = {
              theme = "Dracula";
              inputModel = "Modal";   # or "KeyTrie" / "Standard"
              font.sizePx = 15;
            };
          };
        }
      ];
    };
  };
}
```

## Warnings

- **Secrets:** Nix settings land in the world-readable Nix store and get
  compiled into the binary. Do **not** declare an assistant API-key
  literal here; the shipped default reads `ERAX_API_KEY` from the
  environment at runtime for exactly this reason.
- This layer builds erax from source on your machine. That is not a
  concession, it's the product: your editor, your consts, your binary.

## Status

Designed and scaffolded alongside the main patch. The option set covers
the headline settings; growing it is mechanical (one option in
`module.nix`, one override entry in `lib.nix`). It has not yet been
exercised in a full `nixos-rebuild` cycle — the anchors target the current
`src/config/` module layout and fail safe if that layout drifts.
