{
  description = "Nixfied v2 greenfield workspace";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-25.05";
    # Pinned Rust toolchain source. nixpkgs 25.05 ships an rustc older than the
    # workspace's rust-version (let-chains), so the runtime/conformance binaries
    # are built from a toolchain pinned here: host-Rust-free and reproducible.
    rust-overlay = {
      url = "github:oxalica/rust-overlay";
      inputs.nixpkgs.follows = "nixpkgs";
    };
  };

  outputs =
    {
      self,
      nixpkgs,
      rust-overlay,
    }:
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
            pkgs = import nixpkgs {
              inherit system;
              overlays = [ rust-overlay.overlays.default ];
            };
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
          # The nix-built runtime/conformance binaries (host-Rust-free), shared
          # with the self-project conformance workflow.
          nixfiedRuntime = import ./nix/packages/runtime.nix { inherit pkgs; };
          minimalModel = nixfiedLib.compileModel ./examples/minimal/nixfied.nix;
          postgresModel = nixfiedLib.compileModel ./examples/postgres/nixfied.nix;
          workflowModel = nixfiedLib.compileModel ./examples/workflow/nixfied.nix;
          polyglotModel = nixfiedLib.compileModel ./examples/polyglot-stack/nixfied.nix;
          downstreamModel = nixfiedLib.compileModel ./examples/downstream/nixfied.nix;
          # The framework's own project: a `conformance` workflow that drives the
          # examples + adoption through the nix-built runtime. This is the gate.
          selfModel = nixfiedLib.compileModel ./nixfied.nix;
          nixfiedInstall = import ./nix/install/install.nix { inherit pkgs; };
          nixfiedUpgrade = import ./nix/install/upgrade.nix { inherit pkgs; };
          # `nix run .#gate`: launch the self-hosted conformance gate against the
          # current working tree (rebuilds the runtime + self-model on each run).
          nixfiedGate = import ./nix/gate.nix {
            inherit pkgs;
            runtime = nixfiedRuntime;
            model = selfModel;
          };
        in
        {
          default = minimalModel;
          nixfied-runtime = nixfiedRuntime;
          install = nixfiedInstall;
          upgrade = nixfiedUpgrade;
          gate = nixfiedGate;
          minimal-model = minimalModel;
          postgres-model = postgresModel;
          workflow-model = workflowModel;
          polyglot-stack-model = polyglotModel;
          downstream-model = downstreamModel;
          self-model = selfModel;
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
          gate = {
            type = "app";
            program = "${self.packages.${system}.gate}/bin/nixfied-gate";
            meta.description = "Run the self-hosted conformance gate against the working tree";
          };
        }
      );

      checks = forAllSystems (
        { pkgs, system }:
        {
          minimal-model = self.packages.${system}.minimal-model;
          # The nix-packaged runtime binary must compile reproducibly.
          nixfied-runtime = self.packages.${system}.nixfied-runtime;
          # The self-project conformance model must build (the gate's input).
          self-model = self.packages.${system}.self-model;
          rust-workspace = pkgs.runCommand "nixfied-rust-workspace-check" { } ''
            mkdir -p "$out"
          '';
        }
      );

      devShells = forAllSystems (
        { pkgs, ... }:
        {
          # The pinned toolchain (cargo/rustc/clippy/rustfmt) so the cargo floor
          # runs identically on every host and in CI, independent of host Rust.
          default = pkgs.mkShell {
            packages = [
              (pkgs.rust-bin.stable."1.96.0".minimal.override {
                extensions = [
                  "clippy"
                  "rustfmt"
                ];
              })
              pkgs.sqlite
              pkgs.nix
              pkgs.git
            ];
          };
        }
      );
    };
}
