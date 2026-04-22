{
  pkgs,
  model,
  services,
}:
let
  runtimeFixture = import ./lib/runtime-fixture.nix { inherit pkgs; };
  shellHelpers = import ./lib/shell-helpers.nix { inherit pkgs; };
  baseTask = model.tasks."task.ci.quality";
  probeTaskId = "task.test.logging-injection.probe";
  probeTask = baseTask // {
    id = probeTaskId;
    summary = "logging alias conflict probe";
    description = "Exercises env-sandbox logging and output alias resolution locally.";
    runner = {
      type = "shell";
      command = ''
        set -euo pipefail
        echo "INFO: resolved_log_level=$LOG_LEVEL"
        echo "INFO: resolved_output_mode=$OUTPUT_MODE"
        echo "OK: logging probe complete"
      '';
      package = null;
      workflowId = null;
    };
  };
  probeModel = runtimeFixture.withCompiledExecution (
    model
    // {
      tasks = model.tasks // {
        ${probeTaskId} = probeTask;
      };
    }
  );
  probeExecutionTask = probeModel.compiled.execution.tasks.byId.${probeTaskId};
  inherit (probeExecutionTask) runtimePlanShell;
  envSandboxShell = import ../../nixfied/framework/runtime/env-sandbox.nix {
    inherit
      pkgs
      services
      ;
    model = probeModel;
    projectRoot = ../..;
  };
in
pkgs.runCommand "logging-injection-smoke" { } ''
  set -euo pipefail
  ${shellHelpers.shellPrelude}
  ${envSandboxShell}

  export NIXFIED_RUNTIME_DIR_SCOPE_OVERRIDE="$TMPDIR/runtime-scope"
  mkdir -p "$NIXFIED_RUNTIME_DIR_SCOPE_OVERRIDE"

  NIXFIED_LOG_LEVEL=debug \
  NIXFIED_OUTPUT_MODE=both \
    run_in_sandbox_runtime \
      ${pkgs.lib.escapeShellArg runtimePlanShell} \
      ${pkgs.lib.escapeShellArg probeTask.runner.command} > "$TMPDIR/resolved.out" 2>&1 || {
      cat "$TMPDIR/resolved.out"
      fail "expected alias-only logging overrides to resolve in the env sandbox"
    }

  require_contains "$TMPDIR/resolved.out" "INFO: resolved_log_level=debug"
  require_contains "$TMPDIR/resolved.out" "INFO: resolved_output_mode=both"
  require_contains "$TMPDIR/resolved.out" "OK: logging probe complete"

  set +e
  LOG_LEVEL=info \
  NIXFIED_LOG_LEVEL=debug \
    run_in_sandbox_runtime \
      ${pkgs.lib.escapeShellArg runtimePlanShell} \
      ${pkgs.lib.escapeShellArg probeTask.runner.command} > "$TMPDIR/log-level-conflict.out" 2>&1
  conflict_rc="$?"
  set -e
  if [ "$conflict_rc" -eq 0 ]; then
    fail "expected conflicting LOG_LEVEL aliases to fail"
  fi
  require_contains "$TMPDIR/log-level-conflict.out" "LOG_LEVEL and NIXFIED_LOG_LEVEL conflict"

  set +e
  OUTPUT_MODE=stdout \
  NIXFIED_OUTPUT_MODE=logs \
    run_in_sandbox_runtime \
      ${pkgs.lib.escapeShellArg runtimePlanShell} \
      ${pkgs.lib.escapeShellArg probeTask.runner.command} > "$TMPDIR/output-mode-conflict.out" 2>&1
  conflict_output_rc="$?"
  set -e
  if [ "$conflict_output_rc" -eq 0 ]; then
    fail "expected conflicting OUTPUT_MODE aliases to fail"
  fi
  require_contains "$TMPDIR/output-mode-conflict.out" "OUTPUT_MODE and NIXFIED_OUTPUT_MODE conflict"

  echo "OK: env sandbox resolves logging aliases and rejects conflicting invocation aliases" > "$out"
''
