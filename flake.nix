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
          # (run/ps/down/clean/model-check) plus one app per task id the adopter
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
          persistentEndpointModel = nixfiedLib.compileModel (
            { pkgs, ... }:
            {
              imports = [ ./examples/minimal/nixfied.nix ];
              nixfied.tasks.endpoint-prepare = {
                invocation = {
                  tools = [
                    pkgs.bash
                    pkgs.coreutils
                  ];
                  run = [
                    "bash"
                    "-c"
                    ''
                      set -euo pipefail
                      touch "''${stateDir}/endpoint-prepare-sentinel"
                    ''
                  ];
                };
              };
              nixfied.services.synthetic.lifecycle.prepare.task = "endpoint-prepare";
              nixfied.tasks.keep-up = {
                serviceLifetime = "persistent-until-down";
                invocation = {
                  tools = [ "synthetic-helper" ];
                  run = [
                    "nixfied-synthetic-helper"
                    "task"
                    "--host"
                    "127.0.0.1"
                    "--port"
                    "\${port}"
                  ];
                };
                requires = [ "synthetic" ];
              };
            }
          );
          purgeMinimalModel = nixfiedLib.compileModel (
            { ... }:
            {
              imports = [ ./examples/minimal/nixfied.nix ];
              nixfied.state.cleanupPolicy = "protected";
              nixfied.state.persistence = "persistent";
            }
          );
          postgresModel = nixfiedLib.compileModel ./examples/postgres/nixfied.nix;
          # The cargo lifecycle test runs immediately before the gate in `.#ci`.
          # Keep its Postgres window distinct: a stopped server may leave a
          # non-listening TIME_WAIT claim that the gate's raw-bind preflight must
          # continue to refuse rather than treating as available.
          postgresTestModel = nixfiedLib.compileModel (
            { lib, ... }:
            {
              imports = [ ./examples/postgres/nixfied.nix ];
              nixfied.placement.ports.base = lib.mkForce 44580;
            }
          );
          compositeModel = nixfiedLib.compileModel ./examples/composite/nixfied.nix;
          polyglotModel = nixfiedLib.compileModel ./examples/polyglot-stack/nixfied.nix;
          downstreamModel = nixfiedLib.compileModel ./examples/downstream/nixfied.nix;
          # The example shard and the later slot-concurrency shard use distinct
          # deterministic windows. A stopped Postgres can leave a non-listening
          # TIME_WAIT claim that the required raw-bind preflight must refuse as
          # unverifiable rather than silently treating as available.
          downstreamSlotsModel = nixfiedLib.compileModel (
            { lib, ... }:
            {
              imports = [ ./examples/downstream/nixfied.nix ];
              nixfied.placement.ports.base = lib.mkForce 34880;
            }
          );
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
          nixfiedInstall = import ./nix/install/install.nix {
            inherit pkgs;
            # The debug build: the installer only writes scaffold files, and
            # the whole CI loop shares one fast profile.
            runtime = nixfiedRuntimeDebug;
          };
          nixfiedUpgrade = import ./nix/install/upgrade.nix { inherit pkgs; };
          # Nix-layer tests: reject_composite suite + adoption loop.
          nixfiedGateNix = import ./nix/gate-nix.nix {
            inherit pkgs;
            runtime = nixfiedRuntimeDebug;
          };
          # Runtime-layer tests expressed as a first-class nixfied model.
          # The compileModel override injects the runtime closure and all model
          # paths as compile-time env vars.
          gateRuntimeModel = nixfiedLib.compileModel (
            { ... }:
            {
              imports = [ ./nix/gate-runtime/nixfied.nix ];
              nixfied.closures.rt = {
                package = nixfiedRuntimeDebug;
                executable = "bin/nixfied-runtime";
                effects = [
                  "process"
                  "file-write"
                ];
              };
              nixfied.tasks.example-minimal.invocation.env.MINIMAL_MODEL = toString minimalModel;
              nixfied.tasks.example-postgres.invocation.env.POSTGRES_MODEL = toString postgresModel;
              nixfied.tasks.example-composite.invocation.env.COMPOSITE_MODEL = toString compositeModel;
              nixfied.tasks.example-polyglot.invocation.env.POLYGLOT_MODEL = toString polyglotModel;
              nixfied.tasks.example-downstream.invocation.env.DOWNSTREAM_MODEL = toString downstreamModel;
              nixfied.tasks.example-reth.invocation.env.RETH_MODEL = toString rethModel;
              nixfied.tasks.example-toolchain.invocation.env.TOOLCHAIN_MODEL = toString toolchainModel;
              nixfied.tasks.negative-no-selection.invocation.env.MINIMAL_MODEL = toString minimalModel;
              nixfied.tasks.negative-undeclared-task.invocation.env.MINIMAL_MODEL = toString minimalModel;
              nixfied.tasks.negative-failure-identity.invocation.env.NEGATIVE_FAIL_MODEL =
                toString negativeFailModel;
              nixfied.tasks.lifecycle-first-run.invocation.env.MINIMAL_MODEL = toString minimalModel;
              nixfied.tasks.lifecycle-second-run.invocation.env.MINIMAL_MODEL = toString minimalModel;
              nixfied.tasks.lifecycle-upgrade-preserve.invocation.env.MINIMAL_B_MODEL = toString minimalModelB;
              nixfied.tasks.lifecycle-upgrade-epoch.invocation.env.MINIMAL_EPOCH2_MODEL =
                toString minimalModelEpoch2;
              nixfied.tasks.lifecycle-tamper-refusal.invocation.env.MINIMAL_EPOCH2_MODEL =
                toString minimalModelEpoch2;
              nixfied.tasks.lifecycle-service-lifetime.invocation.env.PERSISTENT_ENDPOINT_MODEL =
                toString persistentEndpointModel;
              nixfied.tasks.lifecycle-purge.invocation.env.PURGE_MINIMAL_MODEL = toString purgeMinimalModel;
              nixfied.tasks.endpoint-cross-root.invocation.env.PERSISTENT_ENDPOINT_MODEL =
                toString persistentEndpointModel;
              nixfied.tasks.slot-0.invocation.env.DOWNSTREAM_MODEL = toString downstreamSlotsModel;
              nixfied.tasks.slot-1.invocation.env.DOWNSTREAM_MODEL = toString downstreamSlotsModel;
              nixfied.tasks.slots-assert.invocation.env.DOWNSTREAM_MODEL = toString downstreamSlotsModel;
            }
          );
          nixfiedGateRuntime = pkgs.writeShellApplication {
            name = "nixfied-gate-runtime";
            runtimeInputs = [ nixfiedRuntimeDebug ];
            text = ''
              nixfied-runtime run \
                --model "${gateRuntimeModel}/model.json" \
                --task all \
                --timeout-ms 300000
            '';
          };
          # `nix run .#gate`: the framework gate — runtime-layer tests via the
          # gate-runtime nixfied model, then nix-layer tests via gate-nix.
          nixfiedGate = import ./nix/gate.nix {
            inherit pkgs;
            gateNix = nixfiedGateNix;
            gateRuntime = nixfiedGateRuntime;
          };
          # `.#check` / `.#test` / `.#ci`: the framework's own source/test/CI gate.
          devApps = import ./nix/dev.nix {
            inherit pkgs postgresTestModel;
            gate = nixfiedGate;
            runtime = nixfiedRuntimeDebug;
          };
        in
        {
          default = minimalModel;
          toolchain-model = toolchainModel;
          gate-runtime-model = gateRuntimeModel;
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
          composite-model = compositeModel;
          polyglot-stack-model = polyglotModel;
          downstream-model = downstreamModel;
          reth-model = rethModel;
        }
      );

      apps = forAllSystems (
        { pkgs, system }:
        {
          help = import ./nix/help-app.nix {
            inherit pkgs system;
            flakeRef = self.outPath;
          };
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
        let
          optionsDoc = import ./nix/docs/options.nix {
            inherit pkgs system;
            inherit (nixpkgs) lib;
          };
        in
        {
          minimal-model = self.packages.${system}.minimal-model;
          # The derivation spec's golden vectors as Nix eval fixtures
          # (docs/DERIVATION_SPEC.md §6; DERIVE-1).
          derive-facts-vectors = import ./nix/checks/derive-facts-vectors.nix {
            inherit pkgs;
            inherit (nixpkgs) lib;
          };
          # The runtime workspace must compile reproducibly. CI verifies the fast
          # debug profile for fast iteration. The hosted workflow separately
          # builds the release package as its final safety net.
          nixfied-runtime = self.packages.${system}.nixfied-runtime-debug;
          # Hermetic source gate: checked option reference + rustfmt + clippy
          # (-D warnings). Keep this under the existing check rather than adding
          # another public flake output solely for documentation maintenance.
          rust-workspace = import ./nix/packages/rust-workspace-check.nix {
            inherit pkgs optionsDoc;
          };
        }
      );

      devShells = forAllSystems (
        { pkgs, ... }:
        {
          # The pinned toolchain (cargo/rustc/clippy/rustfmt) so the cargo floor
          # runs identically on every host and in CI, independent of host Rust.
          default = pkgs.mkShell {
            packages = [
              (import ./nix/toolchain.nix { inherit pkgs; }).dev
              pkgs.sqlite
              pkgs.nix
              pkgs.git
              pkgs.python3
            ];
          };
        }
      );
    };
}
