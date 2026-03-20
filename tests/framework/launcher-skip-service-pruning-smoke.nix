{ pkgs }:
let
  lib = pkgs.lib;
  frameworkLib = import ../../nixfied/framework/core {
    inherit pkgs;
    system = pkgs.system;
  };
  shellHelpers = import ./lib/shell-helpers.nix { inherit pkgs; };
  repoRoot = builtins.toString ../..;
  compiledBase = frameworkLib.mkNixfied {
    projectRoot = ../..;
    projectModules = [ ../../nixfied/project/module.nix ];
    extraModules = [ ./launcher-helios-task-module.nix ];
    localOverrides = [ ];
  };
  compiledExcluded = frameworkLib.mkNixfied {
    projectRoot = ../..;
    projectModules = [ ../../nixfied/project/module.nix ];
    extraModules = [ ./launcher-helios-task-module.nix ];
    localOverrides = [
      (
        { ... }:
        {
          nixfied.graph.excludedServices = [ "helios" ];
        }
      )
    ];
  };

  frameworkOutputs = frameworkLib.mkFlakeOutputs {
    projectRoot = ../..;
    projectModules = [ ../../nixfied/project/module.nix ];
    extraModules = [ ./launcher-helios-task-module.nix ];
    localOverrides = [ ];
  };
in
assert builtins.hasAttr "task.test.launcher.helios-required" compiledBase.model.tasks;
assert !(builtins.hasAttr "task.test.launcher.helios-required" compiledExcluded.model.tasks);
assert builtins.hasAttr "task.test.launcher.control" compiledExcluded.model.tasks;
pkgs.runCommand "launcher-skip-service-pruning-smoke" { } ''
  set -euo pipefail
  ${shellHelpers.shellPrelude}

  RUN_TASK_APP="${frameworkOutputs.apps.run-task.program}"

  export REGISTRY_ROOT="$TMPDIR/registry"
  export CI_ARTIFACTS_ROOT="$TMPDIR/ci-artifacts"
  export NIXFIED_RUNTIME_DIR_SCOPE_OVERRIDE="$TMPDIR/runtime-scope"
  export NIXFIED_FLAKE_ROOT=${lib.escapeShellArg repoRoot}
  mkdir -p "$REGISTRY_ROOT" "$CI_ARTIFACTS_ROOT" "$NIXFIED_RUNTIME_DIR_SCOPE_OVERRIDE"

  set +e
  SKIP_HELIOS=1 "$RUN_TASK_APP" task.test.launcher.helios-required > "$TMPDIR/skip-pruned.out" 2>&1
  skip_pruned_rc="$?"
  set -e
  if [ "$skip_pruned_rc" -eq 0 ]; then
    echo "expected SKIP_HELIOS to prune the helios-gated task"
    cat "$TMPDIR/skip-pruned.out"
    exit 1
  fi
  require_contains "$TMPDIR/skip-pruned.out" "ERROR: unknown task 'task.test.launcher.helios-required'"

  SKIP_HELIOS=1 "$RUN_TASK_APP" task.test.launcher.control > "$TMPDIR/skip-control.out" 2>&1
  require_contains "$TMPDIR/skip-control.out" "OK: launcher control task ran"

  echo "OK: launcher SKIP_HELIOS recompiles a pruned graph while leaving unrelated tasks runnable" > "$out"
''
