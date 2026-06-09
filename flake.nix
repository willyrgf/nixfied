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
          postgresModel = nixfiedLib.compileModel ./examples/postgres/nixfied.nix;
          workflowModel = nixfiedLib.compileModel ./examples/workflow/nixfied.nix;
          nixfiedInstall = pkgs.writeShellApplication {
            name = "nixfied-install";
            runtimeInputs = [ pkgs.coreutils ];
            text = builtins.readFile ./nix/install/install.sh;
          };
          nixfiedUpgrade = pkgs.writeShellApplication {
            name = "nixfied-upgrade";
            runtimeInputs = [
              pkgs.coreutils
              pkgs.gnugrep
              pkgs.nix
            ];
            text = builtins.readFile ./nix/install/upgrade.sh;
          };
        in
        {
          default = m0MinimalModel;
          install = nixfiedInstall;
          upgrade = nixfiedUpgrade;
          m0-minimal-model = m0MinimalModel;
          postgres-model = postgresModel;
          workflow-model = workflowModel;
        }
      );

      apps = forAllSystems (
        { system, ... }:
        {
          install = {
            type = "app";
            program = "${self.packages.${system}.install}/bin/nixfied-install";
            meta.description = "Install Nixfied scaffold files into a downstream project";
          };
          upgrade = {
            type = "app";
            program = "${self.packages.${system}.upgrade}/bin/nixfied-upgrade";
            meta.description = "Repin the Nixfied flake input without touching project-owned declarations";
          };
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
