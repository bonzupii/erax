{
  description = "erax — development shell (NOT a packaging flake: erax is configured at compile time, so there is deliberately no packages/apps output for installing erax itself; see nix/config/ for the NixOS configuration layer)";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-26.05";
    rust-overlay = {
      url = "github:oxalica/rust-overlay";
      inputs.nixpkgs.follows = "nixpkgs";
    };
  };

  outputs =
    { nixpkgs, rust-overlay, ... }:
    let
      # Every system this shell is expected to run cleanly on: NixOS and
      # nix-darwin, Intel and Apple Silicon / ARM. (x86_64-darwin exists on
      # the pinned 26.05 branch; it was dropped upstream only in 26.11, so
      # revisit this list if the pin ever moves past that.)
      systems = [
        "x86_64-linux"
        "aarch64-linux"
        "x86_64-darwin"
        "aarch64-darwin"
      ];
      forAllSystems = nixpkgs.lib.genAttrs systems;
      pkgsFor = system: import nixpkgs { inherit system; overlays = [ rust-overlay.overlays.default ]; };
    in
    {
      devShells = forAllSystems (
        system:
        let
          pkgs = pkgsFor system;
          inherit (pkgs) lib;
          isLinux = pkgs.stdenv.isLinux;
          isDarwin = pkgs.stdenv.isDarwin;

          # One toolchain for everything, including the WebUI work: stable
          # Rust with the wasm32 target the web shell compiles to, plus the
          # fmt/clippy components the project's gates use. rust-src is for
          # Miri / -Z build-std work in the verification tiers.
          toolchain = pkgs.rust-bin.stable."1.99.0".default.override {
            extensions = [
              "rust-src"
              "rustfmt"
              "clippy"
              # llvm-cov / llvm-profdata binaries: cargo-llvm-cov drives
              # them for the project's coverage gate.
              "llvm-tools-preview"
            ];
            targets = [ "wasm32-unknown-unknown" ];
          };

          # Fonts are test data, not decoration: the GUI font stack and the
          # complex-script shaping work (N-260) need Latin, CJK and emoji
          # coverage inside the shell.
          fontPackages = with pkgs; [
            fira-code
            jetbrains-mono
            dejavu_fonts
            noto-fonts
            noto-fonts-cjk-sans
            noto-fonts-color-emoji
          ];
          fontDataDirs = builtins.concatStringsSep ":" (map (p: "${p}/share") fontPackages);

          # Native libraries the current dependency tree actually links or
          # dlopens (winit, wgpu, cosmic-text's fontconfig backend,
          # printers). Nothing "just in case": if a dep stops needing one of
          # these, it leaves the list.
          linuxLibs = with pkgs; [
            # winit: X11 + Wayland backends
            libX11
            libXcursor
            libXi
            libXrandr
            libXinerama
            libxcb
            wayland
            libxkbcommon
            # wgpu runtime (Vulkan, with Mesa's software GL/Vulkan for
            # headless and VM testing)
            vulkan-loader
            mesa
            # cosmic-text fontconfig backend, text stack
            fontconfig
            freetype
            harfbuzz
            # printers crate links -lcups
            cups
          ];

          darwinLibs = with pkgs; [
            # printers links CUPS on macOS too; winit/wgpu use the system
            # SDK frameworks (AppKit/Metal/...), which stdenv provides.
            cups
            libiconv
          ];
        in
        {
          default = pkgs.mkShell {
            packages =
              [
                toolchain
                pkgs.pkg-config
                # Test and audit tooling: python3 runs the test corpus
                # generators (tests/gen_vim_corpus.py); semgrep runs the
                # maintained sin-pattern ruleset that gates regressions
                # clippy cannot see (research/ai-code-sins/semgrep.yml in
                # the project ledger, copied into the tree by the audit).
                pkgs.python3
                pkgs.semgrep
                # Coverage: the project targets 100% test coverage;
                # code no test can reach is restructured until it can
                # be reached, or removed.
                pkgs.cargo-llvm-cov
              ]
              ++ fontPackages
              ++ lib.optionals isLinux linuxLibs
              ++ lib.optionals isDarwin darwinLibs;

            # Runtime library resolution for the GUI stack is a Linux
            # concern here: on darwin the shell relies on the SDK and the
            # libraries' install names.
            LD_LIBRARY_PATH = lib.optionalString isLinux (lib.makeLibraryPath linuxLibs);
            FONTCONFIG_FILE = lib.optionalString isLinux "${pkgs.fontconfig.out}/etc/fonts/fonts.conf";
            XDG_DATA_DIRS = lib.optionalString isLinux
              "${fontDataDirs}:${pkgs.shared-mime-info}/share:${pkgs.hicolor-icon-theme}/share";
          };
        }
      );

      formatter = forAllSystems (system: (pkgsFor system).nixfmt);
    };
}
