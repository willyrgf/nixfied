{
  pkgs,
  model,
  registry,
}:
let
  baseTask = model.tasks."task.ci.quality";

  probeTaskId = "task.test.ephemeral.copy-mode";
  probeWorkflowId = "workflow.test.ephemeral.copy-mode";

  probeTask = baseTask // {
    id = probeTaskId;
    summary = "ephemeral copy mode probe";
    description = "Verifies git-files copy mode honors gitignore and subdirectory invocation.";
    runner = {
      type = "shell";
      command = ''
        set -euo pipefail
        echo "INFO: sandbox_pwd=$(pwd -P)"

        required_files=(
          "tracked.txt"
          "tracked-ignored.md"
          "keep-untracked.txt"
        )
        for path in "''${required_files[@]}"; do
          if [ ! -f "$path" ]; then
            echo "missing required file in ephemeral copy: $path"
            exit 1
          fi
        done

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

        echo "OK: copy mode probe task complete"
      '';
      package = null;
      workflowId = null;
    };
    ui = baseTask.ui // {
      app = baseTask.ui.app // {
        expose = false;
        name = "test-ephemeral-copy-mode";
      };
    };
  };

  probeUnit = {
    taskId = probeTaskId;
    needs = [ ];
    locks = [ ];
    when = {
      envEquals = { };
      envPresent = [ ];
    };
    skipIfMissingEnv = [ ];
  };

  probeWorkflow = {
    id = probeWorkflowId;
    summary = "ephemeral copy mode probe workflow";
    description = "Validates git-files copy semantics in ephemeral execution.";
    mode = "custom";
    maxWorkers = 1;
    units = {
      probe = probeUnit;
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
      (
        {
          name = "probe";
        }
        // probeUnit
      )
    ];
  };

  probeModel = model // {
    runtime = model.runtime // {
      ephemeral = (model.runtime.ephemeral or { }) // {
        copyMode = "git-files";
      };
    };
    tasks = model.tasks // {
      ${probeTaskId} = probeTask;
    };
    workflows = model.workflows // {
      ${probeWorkflowId} = probeWorkflow;
    };
  };

  orchestrator = import ../../nixfied/runner/orchestrator.nix {
    inherit
      pkgs
      registry
      ;
    model = probeModel;
    projectRoot = ../..;
  };
in
pkgs.runCommand "ephemeral-copy-mode-smoke" { } ''
  set -euo pipefail

  ORCH="${orchestrator}/bin/nixfied-orchestrator"
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

  set +e
  NIXFIED_CALLER_PWD="$repo/subdir" "$ORCH" run-workflow "${probeWorkflowId}" --summary > "$TMPDIR/probe.out" 2>&1
  probe_rc="$?"
  set -e
  if [ "$probe_rc" -ne 0 ]; then
    echo "probe workflow failed rc=$probe_rc"
    cat "$TMPDIR/probe.out"
    exit 1
  fi

  sandbox_pwd="$(${pkgs.gnused}/bin/sed -n 's/^INFO: sandbox_pwd=//p' "$TMPDIR/probe.out" | ${pkgs.coreutils}/bin/tail -n 1)"
  if ! printf '%s\n' "$sandbox_pwd" | ${pkgs.gnugrep}/bin/grep -Eq '.+-ephemeral-.+/source$'; then
    echo "expected task workdir inside ephemeral source copy"
    echo "sandbox_pwd=$sandbox_pwd"
    cat "$TMPDIR/probe.out"
    exit 1
  fi

  ${pkgs.gnugrep}/bin/grep -Fq "INFO: Using git-files copy mode" "$TMPDIR/probe.out"
  ${pkgs.gnugrep}/bin/grep -Fq "OK: copy mode probe task complete" "$TMPDIR/probe.out"

  echo "OK: ephemeral git-files copy mode honors gitignore and repo root resolution" > "$out"
''
