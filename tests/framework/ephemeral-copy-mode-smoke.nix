{
  pkgs,
  model,
  registry,
}:
let
  baseTask = model.tasks."task.ci.quality";

  mkProbeTask =
    {
      taskId,
      appName,
      expectUntracked,
    }:
    baseTask
    // {
      id = taskId;
      summary = "ephemeral copy mode probe";
      description = "Verifies reproducible tracked-only and explicit worktree copy modes.";
      runner = {
        type = "shell";
        command = ''
          set -euo pipefail
          echo "INFO: sandbox_pwd=$(pwd -P)"

          required_files=(
            "tracked.txt"
            "tracked-ignored.md"
          )
          for path in "''${required_files[@]}"; do
            if [ ! -f "$path" ]; then
              echo "missing required file in ephemeral copy: $path"
              exit 1
            fi
          done

          if [ "${if expectUntracked then "1" else "0"}" = "1" ]; then
            if [ ! -f "keep-untracked.txt" ]; then
              echo "expected explicit worktree copy to include keep-untracked.txt"
              exit 1
            fi
          else
            if [ -e "keep-untracked.txt" ]; then
              echo "expected tracked-only copy to exclude keep-untracked.txt"
              exit 1
            fi
          fi

          excluded_paths=(
            "ignored.tmp"
            "ignored-dir/file.txt"
          )
          for path in "''${excluded_paths[@]}"; do
            if [ -e "$path" ]; then
              echo "ignored path leaked into ephemeral copy: $path"
              exit 1
            fi
          done

          echo "OK: copy mode probe task complete expect_untracked=${if expectUntracked then "1" else "0"}"
        '';
        package = null;
        workflowId = null;
      };
      ui = baseTask.ui // {
        app = baseTask.ui.app // {
          expose = false;
          name = appName;
        };
      };
    };

  mkProbeWorkflow =
    {
      workflowId,
      taskId,
    }:
    {
      id = workflowId;
      summary = "ephemeral copy mode probe workflow";
      description = "Validates reproducible tracked-only and explicit worktree copy semantics in ephemeral execution.";
      mode = "custom";
      maxWorkers = 1;
      units = {
        probe = {
          inherit taskId;
          needs = [ ];
          locks = [ ];
          when = {
            envEquals = { };
            envPresent = [ ];
          };
          skipIfMissingEnv = [ ];
        };
      };
      stages = [ [ "probe" ] ];
      preRun = {
        tasks = [ ];
      };
      postRun = {
        tasks = [ ];
        alwaysRun = true;
      };
      artifacts = {
        root = "/tmp/ci-artifacts";
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
        {
          name = "probe";
          inherit taskId;
          needs = [ ];
          locks = [ ];
          when = {
            envEquals = { };
            envPresent = [ ];
          };
          skipIfMissingEnv = [ ];
        }
      ];
    };

  trackedTaskId = "task.test.ephemeral.copy-mode.tracked";
  trackedWorkflowId = "workflow.test.ephemeral.copy-mode.tracked";
  worktreeTaskId = "task.test.ephemeral.copy-mode.worktree";
  worktreeWorkflowId = "workflow.test.ephemeral.copy-mode.worktree";

  trackedModel = model // {
    runtime = model.runtime // {
      ephemeral = (model.runtime.ephemeral or { }) // {
        copyMode = "git-files";
        includeUntracked = false;
      };
    };
    tasks = model.tasks // {
      ${trackedTaskId} = mkProbeTask {
        taskId = trackedTaskId;
        appName = "test-ephemeral-copy-mode-tracked";
        expectUntracked = false;
      };
    };
    workflows = model.workflows // {
      ${trackedWorkflowId} = mkProbeWorkflow {
        workflowId = trackedWorkflowId;
        taskId = trackedTaskId;
      };
    };
  };

  worktreeModel = model // {
    runtime = model.runtime // {
      ephemeral = (model.runtime.ephemeral or { }) // {
        copyMode = "git-files";
        includeUntracked = true;
      };
    };
    tasks = model.tasks // {
      ${worktreeTaskId} = mkProbeTask {
        taskId = worktreeTaskId;
        appName = "test-ephemeral-copy-mode-worktree";
        expectUntracked = true;
      };
    };
    workflows = model.workflows // {
      ${worktreeWorkflowId} = mkProbeWorkflow {
        workflowId = worktreeWorkflowId;
        taskId = worktreeTaskId;
      };
    };
  };

  trackedOrchestrator = import ../../nixfied/runner/orchestrator.nix {
    inherit
      pkgs
      registry
      ;
    model = trackedModel;
    projectRoot = ../..;
  };

  worktreeOrchestrator = import ../../nixfied/runner/orchestrator.nix {
    inherit
      pkgs
      registry
      ;
    model = worktreeModel;
    projectRoot = ../..;
  };
in
pkgs.runCommand "ephemeral-copy-mode-smoke" { } ''
  set -euo pipefail

  TRACKED_ORCH="${trackedOrchestrator}/bin/nixfied-orchestrator"
  WORKTREE_ORCH="${worktreeOrchestrator}/bin/nixfied-orchestrator"
  export REGISTRY_ROOT="$TMPDIR/registry"
  mkdir -p "$REGISTRY_ROOT"

  repo="$TMPDIR/repo"
  mkdir -p "$repo/subdir" "$repo/ignored-dir"
  printf 'ignored.tmp\nignored-dir/\n*.md\n!tracked-ignored.md\n' > "$repo/.gitignore"
  printf 'tracked\n' > "$repo/tracked.txt"
  printf 'tracked ignored\n' > "$repo/tracked-ignored.md"
  printf 'keep me\n' > "$repo/keep-untracked.txt"
  printf 'ignore me\n' > "$repo/ignored.tmp"
  printf 'ignore dir\n' > "$repo/ignored-dir/file.txt"

  ${pkgs.git}/bin/git -C "$repo" init >/dev/null 2>&1
  ${pkgs.git}/bin/git -C "$repo" add .gitignore tracked.txt tracked-ignored.md

  run_probe() {
    local label="$1"
    local orch="$2"
    local workflow_id="$3"
    local expected_include="$4"
    local out_file="$TMPDIR/$label.out"
    local sandbox_pwd
    local rc

    set +e
    NIXFIED_CALLER_PWD="$repo/subdir" "$orch" run-workflow "$workflow_id" --summary > "$out_file" 2>&1
    rc="$?"
    set -e
    if [ "$rc" -ne 0 ]; then
      echo "probe workflow failed label=$label rc=$rc"
      cat "$out_file"
      exit 1
    fi

    sandbox_pwd="$(${pkgs.gnused}/bin/sed -n 's/^INFO: sandbox_pwd=//p' "$out_file" | ${pkgs.coreutils}/bin/tail -n 1)"
    if ! printf '%s\n' "$sandbox_pwd" | ${pkgs.gnugrep}/bin/grep -Eq '.+-ephemeral-.+/source$'; then
      echo "expected task workdir inside ephemeral source copy label=$label"
      echo "sandbox_pwd=$sandbox_pwd"
      cat "$out_file"
      exit 1
    fi

    ${pkgs.gnugrep}/bin/grep -Fq "INFO: Using git-files copy mode include_untracked=$expected_include" "$out_file"
    ${pkgs.gnugrep}/bin/grep -Fq "OK: copy mode probe task complete expect_untracked=$expected_include" "$out_file"
  }

  run_probe tracked-only "$TRACKED_ORCH" "${trackedWorkflowId}" 0
  run_probe worktree "$WORKTREE_ORCH" "${worktreeWorkflowId}" 1

  echo "OK: ephemeral copy modes separate reproducible tracked-only and explicit worktree behavior" > "$out"
''
