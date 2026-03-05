{ pkgs ? import <nixpkgs> {} }:
let
  fontPackages = with pkgs; [
    fira-code
    jetbrains-mono
    dejavu_fonts
    noto-fonts
    noto-fonts-cjk-sans
    noto-fonts-color-emoji
  ];
  fontDataDirs = builtins.concatStringsSep ":" (map (p: "${p}/share") fontPackages);
in
pkgs.mkShell {
  packages = with pkgs; [
    rustc
    cargo
    pkg-config
    wayland
    libxkbcommon
    vulkan-loader
    mesa
    fontconfig
    freetype
    harfbuzz
    xorg.libX11
    xorg.libXcursor
    xorg.libXi
    xorg.libXrandr
    xorg.libXinerama
    xorg.libxcb
  ] ++ fontPackages;

  LD_LIBRARY_PATH = pkgs.lib.makeLibraryPath [
    pkgs.wayland
    pkgs.libxkbcommon
    pkgs.vulkan-loader
    pkgs.mesa
    pkgs.fontconfig
    pkgs.freetype
    pkgs.harfbuzz
    pkgs.xorg.libX11
    pkgs.xorg.libXcursor
    pkgs.xorg.libXi
    pkgs.xorg.libXrandr
    pkgs.xorg.libXinerama
    pkgs.xorg.libxcb
  ];

  FONTCONFIG_FILE = "${pkgs.fontconfig.out}/etc/fonts/fonts.conf";
  XDG_DATA_DIRS = "${fontDataDirs}:${pkgs.shared-mime-info}/share:${pkgs.hicolor-icon-theme}/share";
}
