{
  description = "Nixfied v2 workspace";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-25.05";
    # Pinned Rust toolchain source. nixpkgs 25.05 ships an rustc older than the
    # workspace's rust-version (let-chains), so the runtime/CLI binaries are built
    # from a toolchain pinned here: host-Rust-free and reproducible.
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
        let
          compileModel =
            module:
            import ./nix/compiler/default.nix {
              inherit (nixpkgs) lib;
              inherit pkgs system module;
            };
          # The nix-built runtime an adopter's apps run against. Lazy: only forced
          # when `projectApps` is used, so `compileModel`-only callers don't build it.
          runtime = import ./nix/packages/runtime.nix { inherit pkgs; };
          composeLib = import ./nix/lib/compose.nix { inherit (nixpkgs) lib; };
          resolveConfig =
            module:
            (import ./nix/compiler/resolve.nix {
              inherit (nixpkgs) lib;
              inherit pkgs system module;
            }).config;
        in
        {
          inherit compileModel;
          inherit (composeLib) seq;
          # The generated project surface: the reserved control apps
          # (run/ps/down/clean/admit) plus one app per task id the adopter
          # exports in `nixfied.surface.verbs`.
          projectApps =
            module:
            import ./nix/project-apps.nix {
              inherit pkgs runtime;
              inherit (nixpkgs) lib;
              model = compileModel module;
              config = resolveConfig module;
            };
        };
    in
    {
      lib = forAllSystems mkNixfiedLib;

      packages = forAllSystems (
        { pkgs, system }:
        let
          nixfiedLib = mkNixfiedLib { inherit pkgs system; };
          # The release runtime/CLI binaries (host-Rust-free) — what `.#install`
          # ships to adopters and what `projectApps` runs.
          nixfiedRuntime = import ./nix/packages/runtime.nix { inherit pkgs; };
          # The debug build the framework's own CI path uses (flake checks and the
          # gate), so every `.#ci` compile shares one fast profile instead of also
          # building release optimization.
          nixfiedRuntimeDebug = import ./nix/packages/runtime.nix {
            inherit pkgs;
            buildType = "debug";
          };
          minimalModel = nixfiedLib.compileModel ./examples/minimal/nixfied.nix;
          postgresModel = nixfiedLib.compileModel ./examples/postgres/nixfied.nix;
          workflowModel = nixfiedLib.compileModel ./examples/workflow/nixfied.nix;
          polyglotModel = nixfiedLib.compileModel ./examples/polyglot-stack/nixfied.nix;
          downstreamModel = nixfiedLib.compileModel ./examples/downstream/nixfied.nix;
          rethModel = nixfiedLib.compileModel ./examples/reth/nixfied.nix;
          toolchainModel = nixfiedLib.compileModel ./examples/toolchain/nixfied.nix;
          # Gate-only variants of the example models, for the state lifecycle
          # shard: a provenance-only delta (same identity, new model hash), an
          # epoch bump (declared state-compatibility boundary), and a postgres
          # whose smoke query sleeps long enough to interrupt mid-run.
          minimalModelB = nixfiedLib.compileModel (
            { lib, ... }:
            {
              imports = [ ./examples/minimal/nixfied.nix ];
              nixfied.project.name = lib.mkForce "Minimal B";
            }
          );
          minimalModelEpoch2 = nixfiedLib.compileModel (
            { ... }:
            {
              imports = [ ./examples/minimal/nixfied.nix ];
              nixfied.state.stateEpoch = "2";
            }
          );
          # A deterministically failing composite (its leaf dials a closed
          # port), for the gate's failure-identity negative check.
          negativeFailModel = nixfiedLib.compileModel (
            { ... }:
            {
              imports = [ ./examples/minimal/nixfied.nix ];
              nixfied.tasks.always-fails = {
                operationId = "task.always-fails.run";
                invocation = {
                  tools = [ "synthetic-helper" ];
                  run = [
                    "nixfied-synthetic-helper"
                    "task"
                    "--host"
                    "127.0.0.1"
                    "--port"
                    "1"
                  ];
                };
              };
              nixfied.tasks.failing = {
                kind = "composite";
                steps.boom.task = "always-fails";
              };
            }
          );
          postgresSlowModel = nixfiedLib.compileModel (
            { lib, ... }:
            {
              imports = [ ./examples/postgres/nixfied.nix ];
              nixfied.tasks.smoke-query.invocation.run = lib.mkForce [
                "psql"
                "-h"
                "127.0.0.1"
                "-p"
                "\${port}"
                "-U"
                "postgres"
                "-d"
                "postgres"
                "-w"
                "-tAc"
                "SELECT pg_sleep(120)"
              ];
            }
          );
          nixfiedInstall = import ./nix/install/install.nix { inherit pkgs; };
          nixfiedUpgrade = import ./nix/install/upgrade.nix { inherit pkgs; };
          # `nix run .#gate`: the framework gate — runs the example models directly
          # (rebuilds the debug runtime + models from the working tree on each run).
          nixfiedGate = import ./nix/gate.nix {
            inherit pkgs;
            runtime = nixfiedRuntimeDebug;
            models = {
              minimal = minimalModel;
              postgres = postgresModel;
              workflow = workflowModel;
              polyglot = polyglotModel;
              downstream = downstreamModel;
              reth = rethModel;
              toolchain = toolchainModel;
              minimalB = minimalModelB;
              minimalEpoch2 = minimalModelEpoch2;
              postgresSlow = postgresSlowModel;
              negativeFail = negativeFailModel;
            };
          };
          # `.#check` / `.#test` / `.#ci`: the framework's own source/test/CI gate.
          devApps = import ./nix/dev.nix {
            inherit pkgs;
            gate = nixfiedGate;
            runtime = nixfiedRuntimeDebug;
          };
        in
        {
          default = minimalModel;
          toolchain-model = toolchainModel;
          nixfied-runtime = nixfiedRuntime;
          # The debug runtime the framework's CI path builds (also the
          # `nixfied-runtime` check). Adopters never use this; `.#install` ships
          # the release `nixfied-runtime` above.
          nixfied-runtime-debug = nixfiedRuntimeDebug;
          install = nixfiedInstall;
          upgrade = nixfiedUpgrade;
          gate = nixfiedGate;
          check = devApps.check;
          test = devApps.test;
          ci = devApps.ci;
          minimal-model = minimalModel;
          postgres-model = postgresModel;
          workflow-model = workflowModel;
          polyglot-stack-model = polyglotModel;
          downstream-model = downstreamModel;
          reth-model = rethModel;
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
            meta.description = "Run the framework gate (examples + slots/negative/adoption) against the working tree";
          };
          check = {
            type = "app";
            program = "${self.packages.${system}.check}/bin/nixfied-check";
            meta.description = "Hermetic source gate (rustfmt/clippy/check) + model admission";
          };
          test = {
            type = "app";
            program = "${self.packages.${system}.test}/bin/nixfied-test";
            meta.description = "Run the white-box cargo test floor with the pinned toolchain";
          };
          ci = {
            type = "app";
            program = "${self.packages.${system}.ci}/bin/nixfied-ci";
            meta.description = "Fail-fast whole-repo gate: check then test then the gate";
          };
        }
      );

      checks = forAllSystems (
        { pkgs, system }:
        {
          minimal-model = self.packages.${system}.minimal-model;
          # The derivation spec's golden vectors as Nix eval fixtures
          # (docs/DERIVATION_SPEC.md §6; DERIVE-1).
          derive-facts-vectors = import ./nix/checks/derive-facts-vectors.nix {
            inherit pkgs;
            inherit (nixpkgs) lib;
          };
          # The runtime workspace must compile reproducibly. CI verifies the fast
          # debug profile (release is built on demand by `.#install` / the
          # `nixfied-runtime` package); a release-only compile break is essentially
          # impossible once clippy + debug pass.
          nixfied-runtime = self.packages.${system}.nixfied-runtime-debug;
          # Hermetic source gate: rustfmt + clippy (-D warnings) + cargo check.
          rust-workspace = import ./nix/packages/rust-workspace-check.nix { inherit pkgs; };
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
