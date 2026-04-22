{
  pkgs,
  model,
  services,
}:
let
  runtimeFixture = import ./lib/runtime-fixture.nix { inherit pkgs; };
  shellHelpers = import ./lib/shell-helpers.nix { inherit pkgs; };
  baseTask = model.tasks."task.ci.quality";
  probeTaskId = "task.test.caller-pwd.remote-projectroot";
  probeTask = baseTask // {
    id = probeTaskId;
    summary = "remote caller pwd workdir probe";
    description = "Verifies store-backed project-root tasks resolve workdir from the caller checkout.";
    runner = {
      type = "shell";
      command = ''
        set -euo pipefail
        echo "INFO: sandbox_pwd=$(pwd -P)"
        echo "OK: caller pwd probe task complete"
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
pkgs.runCommand "caller-pwd-remote-projectroot-smoke" { } ''
  set -euo pipefail
  ${shellHelpers.shellPrelude}
  ${envSandboxShell}

  export NIXFIED_RUNTIME_DIR_SCOPE_OVERRIDE="$TMPDIR/runtime-scope"
  mkdir -p "$NIXFIED_RUNTIME_DIR_SCOPE_OVERRIDE"

  caller_repo="$TMPDIR/caller-repo"
  mkdir -p "$caller_repo/subdir"

  NIXFIED_CALLER_PWD="$caller_repo/subdir" \
    run_in_sandbox_runtime \
      ${pkgs.lib.escapeShellArg runtimePlanShell} \
      ${pkgs.lib.escapeShellArg probeTask.runner.command} > "$TMPDIR/probe.out" 2>&1 || {
      cat "$TMPDIR/probe.out"
      fail "caller pwd probe should succeed through env-sandbox workdir resolution"
    }

  sandbox_pwd="$(${pkgs.gnused}/bin/sed -n 's/^INFO: sandbox_pwd=//p' "$TMPDIR/probe.out" | ${pkgs.coreutils}/bin/tail -n 1)"
  if [ "$sandbox_pwd" != "$caller_repo/subdir" ]; then
    echo "sandbox_pwd=$sandbox_pwd"
    cat "$TMPDIR/probe.out"
    fail "expected sandbox workdir to follow caller pwd for store-backed project roots"
  fi

  require_contains "$TMPDIR/probe.out" "OK: caller pwd probe task complete"

  echo "OK: env sandbox resolves store-backed project roots against caller pwd" > "$out"
''
