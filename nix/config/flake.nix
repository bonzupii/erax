{
  description = "erax NixOS configuration layer — declare erax's compile-time config in Nix; a patched, personally-configured erax is built from it. This is a CONFIGURATION flake (for dropping into /etc/nixos), not the dev shell (that's ../../flake.nix) and not a general packaging flake.";

  inputs.nixpkgs.url = "github:NixOS/nixpkgs/nixos-26.05";

  outputs =
    { nixpkgs, ... }:
    let
      eraxLibFor = pkgs: import ./lib.nix { inherit (pkgs) lib; };
    in
    {
      # The patch macros, usable without the module:
      #   erax-config.lib.<system>.patchScript [ { file; name; value; } ... ]
      #   erax-config.lib.<system>.mkPatchedErax { pkgs; src; settings; }
      lib = nixpkgs.lib.genAttrs [ "x86_64-linux" "aarch64-linux" "x86_64-darwin" "aarch64-darwin" ] (
        system: eraxLibFor (import nixpkgs { inherit system; })
      );

      nixosModules.default =
        { pkgs, ... }@args:
        import ./module.nix { eraxLib = eraxLibFor pkgs; } args;
    };
}
