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
    extraModules = [ ../../nixfied/framework/testing/repo-overlay.nix ];
    localOverrides = [ ./lib/test-probe-overrides.nix ];
  };
in
pkgs.runCommand "orchestrator-arg-forwarding-smoke" { } ''
  set -euo pipefail
  ${shellHelpers.shellPrelude}

  RUN_TASK_APP="${frameworkOutputs.apps.run-task.program}"
  TEST_APP="${frameworkOutputs.apps.test.program}"
  JQ="${pkgs.jq}/bin/jq"

  export REGISTRY_ROOT="$TMPDIR/registry"
  export CI_ARTIFACTS_ROOT="$TMPDIR/ci-artifacts"
  export NIXFIED_RUNTIME_DIR_SCOPE_OVERRIDE="$TMPDIR/runtime-scope"
  export NIXFIED_FLAKE_ROOT=${pkgs.lib.escapeShellArg repoRoot}
  mkdir -p "$REGISTRY_ROOT" "$CI_ARTIFACTS_ROOT" "$NIXFIED_RUNTIME_DIR_SCOPE_OVERRIDE"
  cd "$NIXFIED_FLAKE_ROOT"

  "$RUN_TASK_APP" task.test.isolation.unit > "$TMPDIR/run-task.out" 2> "$TMPDIR/run-task.err"
  require_contains "$TMPDIR/run-task.out" "OK: isolation probe complete"
  require_not_contains "$TMPDIR/run-task.err" "jq:"

  "$TEST_APP" --mode feature-proof --summary --summary-file "$TMPDIR/test.summary.json" > "$TMPDIR/test.out" 2> "$TMPDIR/test.err"
  require_file "$TMPDIR/test.summary.json"
  "$JQ" -e '
    .kind == "workflow-summary"
    and .payload.workflow_id == "workflow.test.feature-proof"
    and .payload.mode == "test"
    and .payload.exit_code == 0
  ' "$TMPDIR/test.summary.json" > /dev/null
  require_contains "$TMPDIR/test.out" "OK: Exit code: 0"
  require_not_contains "$TMPDIR/test.err" "jq:"

  echo "OK: orchestrator handles empty and option-like forwarded args without jq noise" > "$out"
''
