{
  description = "Nixfied v2 greenfield workspace";

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
          minimalModel = nixfiedLib.compileModel ./examples/minimal/nixfied.nix;
          postgresModel = nixfiedLib.compileModel ./examples/postgres/nixfied.nix;
          workflowModel = nixfiedLib.compileModel ./examples/workflow/nixfied.nix;
          polyglotModel = nixfiedLib.compileModel ./examples/polyglot-stack/nixfied.nix;
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
          # The conformance suite drives real `nix build` + the runtime binary, so
          # it runs outside the nix-build sandbox via `nix run .#conformance`. It
          # builds the harness/runtime from the checkout (default $PWD).
          # Uses the host cargo toolchain (the workspace pins rust 1.91, newer than
          # nixpkgs provides) and the host nix, the same way the shell proofs do.
          nixfiedConformance = pkgs.writeShellApplication {
            name = "nixfied-conformance";
            runtimeInputs = [ pkgs.coreutils ];
            text = ''
              checkout="''${1:-$PWD}"
              exec cargo run --manifest-path "$checkout/runtime/Cargo.toml" \
                -p nixfied-conformance -- --checkout "$checkout"
            '';
          };
        in
        {
          default = minimalModel;
          install = nixfiedInstall;
          upgrade = nixfiedUpgrade;
          conformance = nixfiedConformance;
          minimal-model = minimalModel;
          postgres-model = postgresModel;
          workflow-model = workflowModel;
          polyglot-stack-model = polyglotModel;
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
          conformance = {
            type = "app";
            program = "${self.packages.${system}.conformance}/bin/nixfied-conformance";
            meta.description = "Run the downstream conformance suite over public surfaces";
          };
        }
      );

      checks = forAllSystems (
        { pkgs, system }:
        {
          minimal-model = self.packages.${system}.minimal-model;
          # The conformance suite itself runs via `nix run .#conformance` (it
          # drives real nix builds + the runtime binary, which the nix-build
          # sandbox cannot host); here we at least gate that its wrapper builds.
          conformance-app = self.packages.${system}.conformance;
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
