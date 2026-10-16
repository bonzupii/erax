# Patch macros for erax's compile-time configuration.
#
# erax has no runtime config file: settings are typed `pub const`s in
# src/config/*.rs. The Nix layer therefore works by PATCHING THOSE CONSTS
# in a copied source tree before cargo builds it. Each override names the
# config module file, the const, and the new literal (Rust syntax, exactly
# as it should appear after the `=`).
#
# The generated substitution is anchored to the const declaration line
# (`pub const NAME: <type> = ...;`) and fails the build loudly if the
# anchor is not found exactly once — config drift in the erax source is a
# build error here, never a silently-unapplied setting.
{ lib }:
let
  # One override -> one sed program line operating on one file.
  #   { file = "theme.rs"; name = "THEME"; value = "ThemeId::Dracula"; }
  substitutionFor =
    o:
    let
      # Escape the const name for use in a sed address/pattern (names are
      # SCREAMING_SNAKE_CASE identifiers, so this is belt and braces).
      name = lib.escapeRegex o.name;
    in
    ''
      if [ "$(grep -cE 'pub const ${name}:' ${o.file})" != "1" ]; then
        echo "erax config patch: expected exactly one 'pub const ${name}' in src/config/${o.file}" >&2
        exit 1
      fi
      sed -i -E 's|(pub const ${name}: [^=]+=).*;|\1 ${o.value};|' ${o.file}
    '';

  # Apply a list of overrides inside a derivation's postPatch phase.
  # Usage in a package override:
  #   postPatch = (old.postPatch or "") + eraxConfigLib.patchScript [
  #     { file = "theme.rs"; name = "THEME"; value = "ThemeId::Dracula"; }
  #   ];
  patchScript =
    overrides:
    ''
      (
        cd src/config
        ${lib.concatMapStrings substitutionFor overrides}
      )
    '';

  # Build erax from `src` with the given settings applied.
  # `settings` is the module's option attrset (see module.nix); the mapping
  # from settings to const overrides lives here so the module stays thin.
  mkPatchedErax =
    { pkgs, src, settings }:
    let
      overrides =
        lib.optional (settings.theme != null) {
          file = "theme.rs";
          name = "THEME";
          value = "ThemeId::${settings.theme}";
        }
        ++ lib.optional (settings.inputModel != null) {
          file = "keys.rs";
          name = "INPUT_MODEL";
          value = "InputModel::${settings.inputModel}";
        }
        ++ lib.optional (settings.font.sizePx != null) {
          file = "font.rs";
          name = "FONT_SIZE_PX";
          # FONT_SIZE_PX is an f32 const in the source; emit a float
          # literal, an integer would (correctly) fail to compile.
          value = "${toString settings.font.sizePx}.0";
        }
        ++ settings.extraOverrides;
    in
    pkgs.rustPlatform.buildRustPackage {
      pname = "erax-configured";
      version = "custom";
      inherit src;
      cargoLock.lockFile = "${src}/Cargo.lock";
      # Native build inputs mirror the dev shell in ../../flake.nix
      # (Linux set shown; extend per platform as the main flake does).
      nativeBuildInputs = [ pkgs.pkg-config ];
      buildInputs = lib.optionals pkgs.stdenv.isLinux (
        with pkgs;
        [
          libX11
          libXcursor
          libXi
          libXrandr
          libXinerama
          libxcb
          wayland
          libxkbcommon
          vulkan-loader
          fontconfig
          freetype
          harfbuzz
          cups
        ]
      );
      postPatch = patchScript overrides;
      # A configuration build produces the user's binary; it does not
      # run the development test suite (the dev shell in ../../flake.nix
      # owns tests and their fixtures). Keeping doCheck off is also
      # what keeps dev-only dependencies out of this flake.
      doCheck = false;
      # A personal, compile-time-configured editor is built FOR the user,
      # on the user's machine, from the user's own settings.
      meta.mainProgram = "erax";
    };
in
{
  inherit patchScript mkPatchedErax;
}
