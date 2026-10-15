# NixOS module: declare your erax configuration in Nix, get an erax binary
# built with exactly those compile-time consts. Drop this flake into your
# /etc/nixos flake inputs and import `nixosModules.default`.
#
#   programs.erax = {
#     enable = true;
#     settings = {
#       theme = "Dracula";
#       inputModel = "Modal";
#       font.sizePx = 15;
#     };
#   };
#
# NOTE on secrets: settings are compiled into the binary AND stored in the
# world-readable Nix store. NEVER put an assistant API-key literal here —
# use the shipped default (the ERAX_API_KEY environment variable) instead.
{ eraxLib }:
{ config, lib, pkgs, ... }:
let
  cfg = config.programs.erax;
in
{
  options.programs.erax = {
    enable = lib.mkEnableOption "erax, configured declaratively at compile time";

    src = lib.mkOption {
      type = lib.types.path;
      description = "erax source tree to build from (your checkout or a pinned flake input).";
    };

    settings = {
      theme = lib.mkOption {
        type = lib.types.nullOr lib.types.str;
        default = null;
        example = "Dracula";
        description = "ThemeId variant name from src/config/theme.rs (e.g. Gruvbox, Dracula). Null keeps the source default. The build fails if the name does not exist — that is the compile-time config philosophy working as intended.";
      };

      inputModel = lib.mkOption {
        type = lib.types.nullOr (lib.types.enum [ "KeyTrie" "Modal" "Standard" ]);
        default = null;
        description = "Input model: KeyTrie (Emacs/uemacs, the default), Modal (Vim), or Standard (Notepad/VS Code CUA). Null keeps the source default.";
      };

      font.sizePx = lib.mkOption {
        type = lib.types.nullOr lib.types.ints.positive;
        default = null;
        example = 15;
        description = "Base font size in px (src/config/font.rs FONT_SIZE_PX). Null keeps the source default.";
      };

      extraOverrides = lib.mkOption {
        type = lib.types.listOf (lib.types.attrsOf lib.types.str);
        default = [ ];
        example = [
          {
            file = "editor.rs";
            name = "TAB_WIDTH";
            value = "2";
          }
        ];
        description = "Escape hatch: raw const overrides as { file, name, value } (see lib.nix). Values are Rust literals, inserted verbatim — invalid Rust is a compile error, by design.";
      };
    };
  };

  config = lib.mkIf cfg.enable {
    environment.systemPackages = [
      (eraxLib.mkPatchedErax {
        inherit pkgs;
        inherit (cfg) src;
        settings = cfg.settings;
      })
    ];
  };
}
