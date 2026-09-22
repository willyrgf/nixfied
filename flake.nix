{
  description = "Nixfied v2 workspace";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixpkgs-unstable";
    # Independently pinned Rust toolchain source. Runtime/CLI compiler upgrades
    # stay separate from nixpkgs updates: host-Rust-free and reproducible.
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
      mkNixfiedPackage =
        {
          pkgs,
          package,
          buildType ? "release",
        }:
        import ./nix/packages/runtime.nix {
          inherit pkgs package buildType;
        };
      libraryDeclarations =
        { pkgs, system }:
        let
          compileModel =
            module:
            import ./nix/compiler/default.nix {
              inherit (nixpkgs) lib;
              inherit pkgs system module;
            };
          # The release runtime an adopter's apps run against. Lazy: only forced
          # when `projectApps` is used, so `compileModel`-only callers don't build it.
          releaseRuntime = mkNixfiedPackage {
            inherit pkgs;
            package = "nixfied-runtime";
          };
          composeLib = import ./nix/lib/compose.nix { inherit (nixpkgs) lib; };
          resolveConfig =
            module:
            (import ./nix/compiler/resolve.nix {
              inherit (nixpkgs) lib;
              inherit pkgs system module;
            }).config;
        in
        [
          {
            kind = "function";
            scope = "library";
            name = "compileModel";
            description = "Compile a native Nixfied module into a model package.";
            input = "A native module function, attribute set, or module path.";
            result = "Derivation containing model.json and disposable views/docs.md.";
            usage = "nixfied.lib.${system}.compileModel ./nixfied.nix";
            references = [
              {
                kind = "topic";
                id = "model";
              }
            ];
            binding = compileModel;
          }
          {
            kind = "function";
            scope = "library";
            name = "seq";
            description = "Construct sequential composite steps from distinct task names.";
            input = "List of distinct declared task identifiers, in execution order.";
            result = "Native steps attribute set; each step depends on its predecessor.";
            usage = ''nixfiedLib.seq [ "lint" "test" ]'';
            references = [
              {
                kind = "topic";
                id = "derivation";
              }
            ];
            binding = composeLib.seq;
          }
          {
            kind = "function";
            scope = "library";
            name = "projectApps";
            description = "Expose discovery, documentation, controls and explicitly exported project tasks.";
            input = "Project-root ./nixfied.nix beside flake.nix and flake.lock.";
            result = "Native flake app attribute set with descriptions.";
            usage = "apps.${system} = nixfied.lib.${system}.projectApps ./nixfied.nix;";
            references = [
              {
                kind = "topic";
                id = "discovery";
              }
              {
                kind = "option";
                path = [
                  "nixfied"
                  "surface"
                  "verbs"
                ];
              }
            ];
            binding =
              module:
              import ./nix/project-apps.nix {
                inherit module;
                inherit pkgs releaseRuntime system;
                inherit (nixpkgs) lib;
                docs = docsFor { inherit pkgs system; };
                publicationTargets = (authoringFor { inherit pkgs system; }).targets;
                model = compileModel module;
                config = resolveConfig module;
              };
          }
        ];
      authoringFor = args: import ./nix/meta/authoring.nix ({ inherit (nixpkgs) lib; } // args);
      publicationFor =
        args@{ pkgs, system }:
        let
          authoring = authoringFor args;
        in
        import ./nix/meta/publications.nix { inherit (nixpkgs) lib; } {
          targets = authoring.targets;
          declarations =
            authoring.declarations
            ++ libraryDeclarations args
            ++ packageDeclarations args
            ++ appDeclarations args
            ++ checkDeclarations args
            ++ shellDeclarations args
            ++ import ./nix/project-publications.nix { };
        };
      publications = forAllSystems publicationFor;
      mkNixfiedLib = { system, ... }: publications.${system}.project "function" "library";
      docsFor =
        args:
        import ./nix/docs/reference.nix (
          {
            inherit (nixpkgs) lib;
            options = (authoringFor args).options;
            publications = publications.${args.system}.entries;
            source = {
              path = toString self.outPath;
              revision = self.rev or null;
              dirtyRevision = self.dirtyRev or null;
              narHash = self.narHash or null;
            };
          }
          // args
        );
      publishPackage = scope: name: description: artifact: binding: {
        kind = "package";
        inherit
          scope
          name
          description
          artifact
          binding
          ;
        usage =
          if scope == "devShell" then
            "nix develop"
          else if scope == "check" then
            "nix build .#checks.<system>.${name}"
          else
            "nix build .#${name}";
      };
      mkNixfiedTestChild =
        pkgs:
        mkNixfiedPackage {
          inherit pkgs;
          package = "nixfied-test-child";
        };
      packageDeclarations =
        { pkgs, system }:
        let
          nixfiedLib = mkNixfiedLib { inherit pkgs system; };
          # Public release products. The installer uses only the CLI; generated
          # project apps use the release runtime through `projectApps` above.
          nixfiedCli = mkNixfiedPackage {
            inherit pkgs;
            package = "nixfied-cli";
          };
          nixfiedRuntime = mkNixfiedPackage {
            inherit pkgs;
            package = "nixfied-runtime";
          };
          # The debug build the framework's own CI path uses (flake checks and the
          # gate), so every `.#ci` compile shares one fast profile instead of also
          # building release optimization.
          nixfiedRuntimeDebug = mkNixfiedPackage {
            inherit pkgs;
            package = "nixfied-runtime";
            buildType = "debug";
          };
          nixfiedTestChild = mkNixfiedTestChild pkgs;
          taskOutputModel = nixfiedLib.compileModel (
            { ... }:
            {
              nixfied.project.projectId = "task-output";
              nixfied.project.name = "Task Output";
              nixfied.codebases.main.logicalRoot = ".";

              nixfied.closures.test-child = {
                package = nixfiedTestChild;
                executable = "bin/nixfied-test-child";
                effects = [ "process" ];
              };

              nixfied.secrets.task-output-secret = {
                source = {
                  kind = "env-var";
                  envVar = "NIXFIED_TASK_OUTPUT_SECRET";
                };
              };

              nixfied.tasks.output = {
                invocation = {
                  tools = [ "test-child" ];
                  run = [
                    "nixfied-test-child"
                    "output"
                    "hex"
                    "00010200ff0a"
                    "646961676e6f73746963"
                  ];
                };
              };

              nixfied.tasks.accepted = {
                invocation = {
                  tools = [ "test-child" ];
                  run = [
                    "nixfied-test-child"
                    "output"
                    "hex-exit"
                    "6163636570746564"
                    "6e6f6e7a65726f"
                    "7"
                  ];
                };
                exitPolicy.successCodes = [
                  0
                  7
                ];
              };

              nixfied.tasks.redacted = {
                invocation = {
                  tools = [ "test-child" ];
                  run = [
                    "nixfied-test-child"
                    "output"
                    "env"
                    "TOKEN"
                  ];
                  env.TOKEN = "\${secret:task-output-secret}";
                };
              };

              nixfied.tasks.timeout = {
                invocation = {
                  tools = [ "test-child" ];
                  run = [
                    "nixfied-test-child"
                    "output"
                    "hex-block"
                    "74696d656f75742d6f7574707574"
                    "74696d656f75742d6572726f72"
                    "\${stateDir}/task-output-timeout-marker"
                  ];
                  timeoutMs = 150;
                };
              };

              nixfied.tasks.cancel = {
                invocation = {
                  tools = [ "test-child" ];
                  run = [
                    "nixfied-test-child"
                    "output"
                    "hex-block"
                    "63616e63656c2d6f7574707574"
                    "63616e63656c2d6572726f72"
                    "\${stateDir}/task-output-cancel-marker"
                  ];
                };
              };

              nixfied.tasks.composite-block = {
                invocation = {
                  tools = [ "test-child" ];
                  run = [
                    "nixfied-test-child"
                    "output"
                    "hex-block"
                    "636f6d706f736974652d6f7574707574"
                    "636f6d706f736974652d6572726f72"
                    "\${stateDir}/task-output-composite-marker"
                  ];
                };
              };

              nixfied.tasks.pipeline = {
                kind = "composite";
                steps.only.task = "composite-block";
              };
            }
          );
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
            cli = nixfiedCli;
          };
          nixfiedUpgrade = import ./nix/install/upgrade.nix { inherit pkgs; };
          # Nix-layer tests: reject_composite suite + adoption loop.
          nixfiedGateNix = import ./nix/gate-nix.nix {
            inherit pkgs;
            debugRuntime = nixfiedRuntimeDebug;
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
              nixfied.tasks.task-output.invocation.env.TASK_OUTPUT_MODEL = toString taskOutputModel;
              nixfied.tasks.task-output.invocation.env.NIXFIED_TASK_OUTPUT_SECRET = "task-output-gate-secret";
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
            debugRuntime = nixfiedRuntimeDebug;
            testChild = nixfiedTestChild;
          };
        in
        [
          (publishPackage "root" "default" "Default minimal example model"
            "model.json and disposable views/docs.md, with realised closure dependencies."
            minimalModel
          )
          (publishPackage "root" "toolchain-model" "Heterogeneous toolchain example model"
            "model.json and disposable views/docs.md, with realised closure dependencies."
            toolchainModel
          )
          (publishPackage "root" "gate-runtime-model" "Runtime integration gate model"
            "model.json and disposable views/docs.md, with realised closure dependencies."
            gateRuntimeModel
          )
          (publishPackage "root" "nixfied-cli" "Release scaffold installer CLI"
            "Executable under bin/ with its native runtime dependencies."
            nixfiedCli
          )
          (publishPackage "root" "nixfied-runtime" "Release generic process runtime"
            "Executable under bin/ with its native runtime dependencies."
            nixfiedRuntime
          )
          (publishPackage "root" "install" "Scaffold installation program"
            "Executable under bin/ with its native runtime dependencies."
            nixfiedInstall
          )
          (publishPackage "root" "upgrade" "Input upgrade and documentation-report program"
            "Executable under bin/ with its native runtime dependencies."
            nixfiedUpgrade
          )
          (publishPackage "root" "gate" "Framework integration gate program"
            "Executable under bin/ with its native runtime dependencies."
            nixfiedGate
          )
          (publishPackage "root" "check" "Hermetic source and model-admission check program"
            "Executable under bin/ with its native runtime dependencies."
            devApps.check
          )
          (publishPackage "root" "test" "Fixture-backed Cargo test program"
            "Executable under bin/ with its native runtime dependencies."
            devApps.test
          )
          (publishPackage "root" "ci" "Whole-repository local CI program"
            "Executable under bin/ with its native runtime dependencies."
            devApps.ci
          )
          (publishPackage "root" "minimal-model" "Minimal service example model"
            "model.json and disposable views/docs.md, with realised closure dependencies."
            minimalModel
          )
          (publishPackage "root" "postgres-model" "PostgreSQL example model"
            "model.json and disposable views/docs.md, with realised closure dependencies."
            postgresModel
          )
          (publishPackage "root" "composite-model" "Composite task example model"
            "model.json and disposable views/docs.md, with realised closure dependencies."
            compositeModel
          )
          (publishPackage "root" "polyglot-stack-model" "Polyglot service example model"
            "model.json and disposable views/docs.md, with realised closure dependencies."
            polyglotModel
          )
          (publishPackage "root" "downstream-model" "Downstream workflow example model"
            "model.json and disposable views/docs.md, with realised closure dependencies."
            downstreamModel
          )
          (publishPackage "root" "reth-model" "Reth multi-endpoint example model"
            "model.json and disposable views/docs.md, with realised closure dependencies."
            rethModel
          )
          (publishPackage "root" "docs" "Revision-bound authoring and API reference"
            "bin/nixfied-docs and share/nixfied/reference/API.md."
            (docsFor {
              inherit pkgs system;
            })
          )
        ];

      appDeclarations =
        { pkgs, system }:
        let
          app =
            name: description: effects: reference: binding:
            {
              kind = "app";
              scope = "root";
              inherit
                name
                description
                effects
                binding
                ;
              usage = "nix run .#${name} -- --help";
            }
            // reference;
          program = name: {
            type = "app";
            program = "${self.packages.${system}.${name}}/bin/nixfied-${name}";
          };
        in
        [
          (app "help" "List this flake's runnable commands"
            "Evaluates final flake app metadata through Nix and verifies source context."
            { topic = "discovery"; }
            (
              import ./nix/help-app.nix {
                inherit pkgs system;
                expectedFlakePath = self.outPath;
                flakeRef = self.outPath;
              }
            )
          )
          (app "docs" "Read the authoring and API reference from this Nixfied source"
            "Reads packaged reference content without model admission, runtime state or network access."
            { topic = "discovery"; }
            (program "docs")
          )
          (app "install" "Install Nixfied scaffold files into a downstream project"
            "Creates scaffold files with native ownership and overwrite checks."
            { command = "install"; }
            (program "install")
          )
          (app "upgrade" "Repin the Nixfied flake input without touching project-owned declarations"
            "Inspects pinned sources and reports documentation changes; checked apply updates project wiring after model preflight."
            { command = "upgrade"; }
            (program "upgrade")
          )
          (app "gate" "Run the framework gate (examples + slots/negative/adoption) against the working tree"
            "Builds and executes runtime and downstream adoption integration fixtures."
            { topic = "development"; }
            (program "gate")
          )
          (app "check" "Hermetic source gate (rustfmt/clippy/check) + model admission"
            "Runs flake checks and admits a realised example model."
            { topic = "development"; }
            (program "check")
          )
          (app "test" "Run the white-box cargo test floor with the pinned toolchain"
            "Executes fixture-backed Cargo tests, including native processes and sockets."
            { topic = "development"; }
            (program "test")
          )
          (app "ci" "Fail-fast whole-repo gate: check then test then the gate"
            "Runs the complete local check, test and integration sequence."
            { topic = "development"; }
            (program "ci")
          )
        ];

      checkDeclarations =
        { pkgs, system }:
        let
          optionsDoc = import ./nix/docs/options.nix {
            inherit pkgs system;
            inherit (nixpkgs) lib;
          };
        in
        [
          (publishPackage "check" "minimal-model" "Build the minimal example model" "Realised model package."
            self.packages.${system}.minimal-model
          )
          (publishPackage "check" "derive-facts-vectors" "Check independent Nix derivation golden vectors"
            "Successful evaluation of the derivation-spec vectors."
            (
              import ./nix/checks/derive-facts-vectors.nix {
                inherit pkgs;
                inherit (nixpkgs) lib;
              }
            )
          )
          (publishPackage "check" "nixfied-runtime" "Compile the debug runtime reproducibly"
            "Debug runtime executable."
            (mkNixfiedPackage {
              inherit pkgs;
              package = "nixfied-runtime";
              buildType = "debug";
            })
          )
          (publishPackage "check" "rust-workspace"
            "Check native metadata, reference freshness, rustfmt and Clippy"
            "Successful hermetic source and type checks."
            (
              assert import ./nix/checks/option-metadata.nix {
                inherit pkgs system;
                inherit (nixpkgs) lib;
              };
              assert import ./nix/checks/publications.nix {
                inherit (nixpkgs) lib;
              };
              assert import ./nix/checks/structure.nix { inherit (nixpkgs) lib; };
              assert import ./nix/checks/syntax.nix { inherit (nixpkgs) lib; };
              assert import ./nix/checks/coverage.nix { inherit (nixpkgs) lib; };
              assert publications.${system}.audit "function" "library" self.lib.${system};
              assert publications.${system}.audit "package" "root" self.packages.${system};
              assert publications.${system}.audit "app" "root" self.apps.${system};
              assert publications.${system}.audit "package" "check" self.checks.${system};
              assert publications.${system}.audit "package" "devShell" self.devShells.${system};
              import ./nix/packages/rust-workspace-check.nix {
                inherit pkgs optionsDoc;
                referenceCheck = import ./nix/checks/reference.nix {
                  inherit pkgs system;
                  inherit (nixpkgs) lib;
                  docs = docsFor { inherit pkgs system; };
                };
              }
            )
          )
        ];

      shellDeclarations =
        { pkgs, ... }:
        let
          nixfiedTestChild = mkNixfiedTestChild pkgs;
        in
        [
          # The pinned toolchain (cargo/rustc/clippy/rustfmt) so the cargo floor
          # runs identically on every host and in CI, independent of host Rust.
          (publishPackage "devShell" "default" "Pinned Rust and Nix development environment"
            "Development shell with the private test-child fixture."
            (
              pkgs.mkShell {
                packages = [
                  (import ./nix/toolchain.nix { inherit pkgs; }).dev
                  pkgs.sqlite
                  pkgs.nix
                  pkgs.git
                ];
                NIXFIED_TEST_CHILD = "${nixfiedTestChild}/bin/nixfied-test-child";
              }
            )
          )
        ];

    in
    {
      lib = forAllSystems mkNixfiedLib;
      packages = forAllSystems ({ system, ... }: publications.${system}.project "package" "root");
      apps = forAllSystems ({ system, ... }: publications.${system}.project "app" "root");
      checks = forAllSystems ({ system, ... }: publications.${system}.project "package" "check");
      devShells = forAllSystems ({ system, ... }: publications.${system}.project "package" "devShell");
    };
}
