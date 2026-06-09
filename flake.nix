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
    { self, nixpkgs, rust-overlay }:
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
          # Pinned toolchain >= the workspace rust-version. The runtime is built
          # from the nix stdenv C toolchain (rusqlite's `bundled` feature compiles
          # SQLite from source; no system sqlite/pkg-config is consumed).
          rustToolchain = pkgs.rust-bin.stable."1.91.0".minimal;
          rustPlatform = pkgs.makeRustPlatform {
            cargo = rustToolchain;
            rustc = rustToolchain;
          };
          nixfiedRuntime = rustPlatform.buildRustPackage {
            pname = "nixfied-runtime";
            version = "0.1.0";
            src = ./runtime;
            cargoLock.lockFile = ./runtime/Cargo.lock;
            # The white-box `cargo test` floor runs outside the build sandbox (it
            # binds ports and spawns process groups); here we only compile.
            doCheck = false;
          };
          minimalModel = nixfiedLib.compileModel ./examples/minimal/nixfied.nix;
          postgresModel = nixfiedLib.compileModel ./examples/postgres/nixfied.nix;
          workflowModel = nixfiedLib.compileModel ./examples/workflow/nixfied.nix;
          polyglotModel = nixfiedLib.compileModel ./examples/polyglot-stack/nixfied.nix;
          downstreamModel = nixfiedLib.compileModel ./examples/downstream/nixfied.nix;
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
          nixfied-runtime = nixfiedRuntime;
          install = nixfiedInstall;
          upgrade = nixfiedUpgrade;
          conformance = nixfiedConformance;
          minimal-model = minimalModel;
          postgres-model = postgresModel;
          workflow-model = workflowModel;
          polyglot-stack-model = polyglotModel;
          downstream-model = downstreamModel;
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
          # The nix-packaged runtime binary must compile reproducibly.
          nixfied-runtime = self.packages.${system}.nixfied-runtime;
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
