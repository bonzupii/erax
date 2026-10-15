# Compatibility shim for `nix-shell` — the development shell itself is
# defined in flake.nix (single source of truth). Prefer `nix develop`.
# This shim requires flakes to be enabled in your Nix config, like any
# getFlake-based shim.
{ system ? builtins.currentSystem }:
let
  flake = builtins.getFlake (toString ./.);
in
flake.devShells.${system}.default
