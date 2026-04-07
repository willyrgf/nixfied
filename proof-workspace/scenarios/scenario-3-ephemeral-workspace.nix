{
  pkgs,
  model,
  services,
  serviceDefinitions,
  registry,
}:
let
  materialize = import ../lib/materialize.nix {
    inherit pkgs;
    repoRoot = ../..;
  };

  baseTask = model.tasks."task.check";
  envFileName = ".ephemeral-secret.env";

  mkProbeTask =
    {
      taskId,
      summary,
      description,
      command,
      runtimeExtra ? { },
    }:
    baseTask
    // {
      id = taskId;
      inherit summary description;
      runner = {
        type = "shell";
        package = null;
        workflowId = null;
        inherit command;
      };
      runtime = (baseTask.runtime or { }) // runtimeExtra;
    };

  mkProbeWorkflow =
    {
      workflowId,
      taskId,
      summary,
      description,
    }:
    let
      unit = {
        inherit taskId;
        needs = [ ];
        locks = [ ];
        when = {
          envEquals = { };
          envPresent = [ ];
        };
        skipIfMissingEnv = [ ];
      };
    in
    {
      id = workflowId;
      inherit summary description;
      mode = "custom";
      maxWorkers = 1;
      units = {
        probe = unit;
      };
      stages = [ [ "probe" ] ];
      preRun = {
        tasks = [ ];
        serviceSets = [ ];
      };
      postRun = {
        tasks = [ ];
        serviceSets = [ ];
        alwaysRun = true;
      };
      artifacts = {
        root = "artifacts-root";
        keepOnSuccess = false;
        keepOnFailure = true;
        writeSummary = true;
      };
      execution = {
        parallel = false;
        failFast = true;
        lockPolicy = "exclusive";
        emitRegistryEvents = true;
        ephemeral = {
          enable = true;
        };
      };
      plan = [
        (
          {
            name = "probe";
          }
          // unit
        )
      ];
    };

  trackedWorkflowId = "workflow.test.proof.scenario3.tracked";
  worktreeWorkflowId = "workflow.test.proof.scenario3.worktree";
  envOriginalRootWorkflowId = "workflow.test.proof.scenario3.env-file.original-root";

  trackedModel = model // {
    runtime = model.runtime // {
      ephemeral = (model.runtime.ephemeral or { }) // {
        copyMode = "git-files";
        includeUntracked = false;
        envFileMode = "disabled";
        envFilePath = envFileName;
      };
    };
    tasks = model.tasks // {
      "task.test.proof.scenario3.tracked" = mkProbeTask {
        taskId = "task.test.proof.scenario3.tracked";
        summary = "proof scenario 3 tracked-only copy mode";
        description = "proof scenario 3 tracked-only copy mode";
        command = ''
          set -euo pipefail
          test -f tracked.txt
          test ! -e keep-untracked.txt
          echo "OK: proof scenario 3 tracked-only copy mode"
        '';
      };
    };
    workflows = model.workflows // {
      ${trackedWorkflowId} = mkProbeWorkflow {
        workflowId = trackedWorkflowId;
        taskId = "task.test.proof.scenario3.tracked";
        summary = "proof scenario 3 tracked-only copy mode";
        description = "proof scenario 3 tracked-only copy mode";
      };
    };
  };

  worktreeModel = model // {
    runtime = model.runtime // {
      ephemeral = (model.runtime.ephemeral or { }) // {
        copyMode = "git-files";
        includeUntracked = true;
        envFileMode = "disabled";
        envFilePath = envFileName;
      };
    };
    tasks = model.tasks // {
      "task.test.proof.scenario3.worktree" = mkProbeTask {
        taskId = "task.test.proof.scenario3.worktree";
        summary = "proof scenario 3 worktree copy mode";
        description = "proof scenario 3 worktree copy mode";
        command = ''
          set -euo pipefail
          test -f tracked.txt
          test -f keep-untracked.txt
          echo "OK: proof scenario 3 worktree copy mode"
        '';
      };
    };
    workflows = model.workflows // {
      ${worktreeWorkflowId} = mkProbeWorkflow {
        workflowId = worktreeWorkflowId;
        taskId = "task.test.proof.scenario3.worktree";
        summary = "proof scenario 3 worktree copy mode";
        description = "proof scenario 3 worktree copy mode";
      };
    };
  };

  envOriginalRootModel = model // {
    runtime = model.runtime // {
      ephemeral = (model.runtime.ephemeral or { }) // {
        copyMode = "git-files";
        includeUntracked = false;
        envFileMode = "original-root";
        envFilePath = envFileName;
      };
    };
    tasks = model.tasks // {
      "task.test.proof.scenario3.env-file.original-root" = mkProbeTask {
        taskId = "task.test.proof.scenario3.env-file.original-root";
        summary = "proof scenario 3 env-file original-root";
        description = "proof scenario 3 env-file original-root";
        command = ''
          set -euo pipefail
          test "''${EPHEMERAL_HOST_ENV_SECRET:-}" = "from-host-env"
          echo "OK: proof scenario 3 env-file original-root"
        '';
        runtimeExtra = {
          passThroughEnv = (baseTask.runtime.passThroughEnv or [ ]) ++ [ "EPHEMERAL_HOST_ENV_SECRET" ];
          allowSensitivePassThrough = true;
        };
      };
    };
    workflows = model.workflows // {
      ${envOriginalRootWorkflowId} = mkProbeWorkflow {
        workflowId = envOriginalRootWorkflowId;
        taskId = "task.test.proof.scenario3.env-file.original-root";
        summary = "proof scenario 3 env-file original-root";
        description = "proof scenario 3 env-file original-root";
      };
    };
  };

  harnessTracked = import ../../tests/framework/lib/harness.nix {
    inherit
      pkgs
      services
      serviceDefinitions
      registry
      ;
    model = trackedModel;
    projectRoot = ../..;
  };

  harnessWorktree = import ../../tests/framework/lib/harness.nix {
    inherit
      pkgs
      services
      serviceDefinitions
      registry
      ;
    model = worktreeModel;
    projectRoot = ../..;
  };

  harnessEnvOriginalRoot = import ../../tests/framework/lib/harness.nix {
    inherit
      pkgs
      services
      serviceDefinitions
      registry
      ;
    model = envOriginalRootModel;
    projectRoot = ../..;
  };
in
pkgs.runCommand "proof-workspace-scenario-3-ephemeral-workspace"
  {
    nativeBuildInputs = [
      pkgs.coreutils
      pkgs.git
      pkgs.gnugrep
      pkgs.gnused
    ];
  }
  ''
    set -euo pipefail
    ${materialize.shellPrelude}

    ORCH_TRACKED="${harnessTracked.orchestrator}/bin/nixfied-orchestrator"
    ORCH_WORKTREE="${harnessWorktree.orchestrator}/bin/nixfied-orchestrator"
    ORCH_ENV_ORIGINAL_ROOT="${harnessEnvOriginalRoot.orchestrator}/bin/nixfied-orchestrator"
    proof_require_file "$ORCH_TRACKED"
    proof_require_file "$ORCH_WORKTREE"
    proof_require_file "$ORCH_ENV_ORIGINAL_ROOT"

    export REGISTRY_ROOT="$TMPDIR/registry"
    export CI_ARTIFACTS_ROOT="$TMPDIR/artifacts"
    mkdir -p "$REGISTRY_ROOT" "$CI_ARTIFACTS_ROOT"

    workspace="$TMPDIR/proof-seed"
    proof_workspace_bootstrap_seed_copy "$workspace"
    printf 'tracked\n' > "$workspace/tracked.txt"
    ${pkgs.git}/bin/git -C "$workspace" add tracked.txt
    ${pkgs.git}/bin/git -C "$workspace" \
      -c user.name=nixfied-proof \
      -c user.email=nixfied-proof@example.invalid \
      commit -m "proof scenario 3 tracked fixture" >/dev/null 2>&1
    printf 'untracked\n' > "$workspace/keep-untracked.txt"
    printf 'EPHEMERAL_HOST_ENV_SECRET=from-host-env\n' > "$workspace/${envFileName}"

    if ! NIXFIED_CALLER_PWD="$workspace" "$ORCH_TRACKED" run-workflow ${trackedWorkflowId} --summary > "$TMPDIR/scenario3.tracked.out" 2>&1; then
      cat "$TMPDIR/scenario3.tracked.out" 2>/dev/null || true
      exit 1
    fi
    proof_require_contains "$TMPDIR/scenario3.tracked.out" "INFO: Skipping host env file import mode=disabled"
    proof_require_contains "$TMPDIR/scenario3.tracked.out" "OK: proof scenario 3 tracked-only copy mode"

    if ! NIXFIED_CALLER_PWD="$workspace" "$ORCH_WORKTREE" run-workflow ${worktreeWorkflowId} --summary > "$TMPDIR/scenario3.worktree.out" 2>&1; then
      cat "$TMPDIR/scenario3.worktree.out" 2>/dev/null || true
      exit 1
    fi
    proof_require_contains "$TMPDIR/scenario3.worktree.out" "OK: proof scenario 3 worktree copy mode"

    if ! NIXFIED_CALLER_PWD="$workspace" "$ORCH_ENV_ORIGINAL_ROOT" run-workflow ${envOriginalRootWorkflowId} --summary > "$TMPDIR/scenario3.env-original-root.out" 2>&1; then
      cat "$TMPDIR/scenario3.env-original-root.out" 2>/dev/null || true
      exit 1
    fi
    proof_require_contains "$TMPDIR/scenario3.env-original-root.out" "INFO: Loading host env file mode=original-root path="
    proof_require_contains "$TMPDIR/scenario3.env-original-root.out" "OK: proof scenario 3 env-file original-root"

    echo "OK: proof workspace scenario 3 ephemeral workspace passed" > "$out"
  ''
