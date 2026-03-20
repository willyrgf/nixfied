{ pkgs }:
let
  lib = pkgs.lib;
  frameworkLib = import ../../nixfied/framework/core {
    inherit pkgs;
    system = pkgs.system;
  };
  shellHelpers = import ./lib/shell-helpers.nix { inherit pkgs; };
  repoRoot = builtins.toString ../..;

  frameworkOutputs = frameworkLib.mkFlakeOutputs {
    projectRoot = ../..;
    projectModules = [ ../../nixfied/project/module.nix ];
    extraModules = [ ];
    localOverrides = [ ./launcher-skip-helios-override.nix ];
  };
in
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
  "$RUN_TASK_APP" task.test.isolation.unit > "$TMPDIR/no-skip.out" 2>&1
  no_skip_rc="$?"
  set -e
  if [ "$no_skip_rc" -eq 0 ]; then
    echo "expected launcher without SKIP_HELIOS to fail"
    cat "$TMPDIR/no-skip.out"
    exit 1
  fi
  require_contains "$TMPDIR/no-skip.out" "helios evaluated unexpectedly"

  SKIP_HELIOS=1 "$RUN_TASK_APP" task.test.isolation.unit > "$TMPDIR/skip.out" 2>&1
  require_contains "$TMPDIR/skip.out" "OK: isolation probe complete"
  require_not_contains "$TMPDIR/skip.out" "helios evaluated unexpectedly"

  echo "OK: launcher SKIP_HELIOS excludes helios before selected app evaluation" > "$out"
''
