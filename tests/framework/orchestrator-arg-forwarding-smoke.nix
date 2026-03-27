{ pkgs }:
let
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
    localOverrides = [ ];
  };
in
pkgs.runCommand "orchestrator-arg-forwarding-smoke" { } ''
  set -euo pipefail
  ${shellHelpers.shellPrelude}

  RUN_TASK_APP="${frameworkOutputs.apps.run-task.program}"
  FRAMEWORK_TEST_APP="${frameworkOutputs.apps."framework::test".program}"

  export REGISTRY_ROOT="$TMPDIR/registry"
  export CI_ARTIFACTS_ROOT="$TMPDIR/ci-artifacts"
  export NIXFIED_RUNTIME_DIR_SCOPE_OVERRIDE="$TMPDIR/runtime-scope"
  export NIXFIED_FLAKE_ROOT=${pkgs.lib.escapeShellArg repoRoot}
  mkdir -p "$REGISTRY_ROOT" "$CI_ARTIFACTS_ROOT" "$NIXFIED_RUNTIME_DIR_SCOPE_OVERRIDE"
  cd "$NIXFIED_FLAKE_ROOT"

  "$RUN_TASK_APP" task.test.isolation.unit > "$TMPDIR/run-task.out" 2> "$TMPDIR/run-task.err"
  require_contains "$TMPDIR/run-task.out" "OK: isolation probe complete"
  require_not_contains "$TMPDIR/run-task.err" "jq:"

  "$FRAMEWORK_TEST_APP" --shard manifest > "$TMPDIR/framework-test.out" 2> "$TMPDIR/framework-test.err"
  require_contains "$TMPDIR/framework-test.out" "OK: framework::test completed"
  require_not_contains "$TMPDIR/framework-test.err" "jq:"

  echo "OK: orchestrator handles empty and option-like forwarded args without jq noise" > "$out"
''
