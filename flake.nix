{
  description = "Nixfied v2 greenfield Milestone 0 workspace";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-25.05";
  };

  outputs =
    { self, nixpkgs }:
    let
      systems = [
        "aarch64-darwin"
        "aarch64-linux"
        "x86_64-darwin"
        "x86_64-linux"
      ];
      forAllSystems =
        f:
        nixpkgs.lib.genAttrs systems (
          system:
          f {
            inherit system;
            pkgs = import nixpkgs { inherit system; };
          }
        );
      mkNixfiedLib =
        { pkgs, system }:
        {
          compileModel =
            module:
            import ./nix/compiler/default.nix {
              inherit (nixpkgs) lib;
              inherit pkgs system module;
            };
        };
    in
    {
      lib = forAllSystems mkNixfiedLib;

      packages = forAllSystems (
        { pkgs, system }:
        let
          nixfiedLib = mkNixfiedLib { inherit pkgs system; };
          m0MinimalModel = nixfiedLib.compileModel ./examples/m0-minimal/nixfied.nix;
        in
        {
          default = m0MinimalModel;
          m0-minimal-model = m0MinimalModel;
        }
      );

      checks = forAllSystems (
        { pkgs, system }:
        {
          m0-minimal-model = self.packages.${system}.m0-minimal-model;
          rust-workspace = pkgs.runCommand "nixfied-rust-workspace-check" { } ''
            mkdir -p "$out"
          '';
        }
      );

      devShells = forAllSystems (
        { pkgs, ... }:
        {
          default = pkgs.mkShell {
            packages = [
              pkgs.cargo
              pkgs.rustc
              pkgs.sqlite
            ];
          };
        }
      );
    };
}
