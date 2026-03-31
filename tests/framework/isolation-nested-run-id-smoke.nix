{
  pkgs,
  model,
  services,
  serviceDefinitions,
  registry,
}:
let
  baseTask = model.tasks."task.check";

  mkShellTask =
    {
      id,
      command,
    }:
    baseTask
    // {
      inherit id;
      summary = id;
      description = id;
      runner = {
        type = "shell";
        inherit command;
        package = null;
        workflowId = null;
      };
      runtime = baseTask.runtime // {
        workdir = "stateRoot";
        customWorkdir = null;
        preHooks = { };
        postHooks = { };
      };
    };

  mkWorkflow =
    {
      id,
      mainTask,
    }:
    {
      inherit id;
      summary = id;
      description = id;
      mode = "custom";
      maxWorkers = 1;
      units = {
        main = {
          taskId = mainTask;
          needs = [ ];
          locks = [ ];
          when = {
            envEquals = { };
            envPresent = [ ];
          };
          skipIfMissingEnv = [ ];
        };
      };
      stages = [ [ "main" ] ];
      preRun = {
        tasks = [ ];
      };
      postRun = {
        tasks = [ ];
        alwaysRun = true;
      };
      artifacts = {
        root = "artifacts-root";
        keepOnSuccess = true;
        keepOnFailure = true;
        writeSummary = true;
      };
      execution = {
        parallel = false;
        failFast = true;
        lockPolicy = "exclusive";
        emitRegistryEvents = true;
        ephemeral = {
          enable = null;
        };
      };
      plan = [
        {
          name = "main";
          taskId = mainTask;
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

  nestedModel = model // {
    tasks = model.tasks // {
      "task.test.nested.child" = mkShellTask {
        id = "task.test.nested.child";
        command = ''
          set -euo pipefail
          echo "OK: nested child workflow ran"
        '';
      };

      "task.test.nested.launcher" = mkShellTask {
        id = "task.test.nested.launcher";
        command = ''
          set -euo pipefail
          nested_dir="$REGISTRY_ROOT/nested-run-id"
          parent_run_file="$nested_dir/parent.run-id"
          child_run_file="$nested_dir/child.run-id"
          child_summary_file="$nested_dir/child.summary.json"
          mkdir -p "$nested_dir"

          printf '%s' "''${NIXFIED_RUN_ID:-}" > "$parent_run_file"
          "$NIXFIED_EXECUTOR_SELF" run-workflow workflow.test.nested.child \
            --run-id-file "$child_run_file" \
            --summary-file "$child_summary_file" \
            --summary > "$nested_dir/child.out" 2>&1

          parent_run_id="$(${pkgs.coreutils}/bin/tr -d '\n' < "$parent_run_file")"
          child_run_id="$(${pkgs.coreutils}/bin/tr -d '\n' < "$child_run_file")"
          if [ -z "$parent_run_id" ] || [ -z "$child_run_id" ]; then
            echo "ERROR: missing nested run id"
            cat "$nested_dir/child.out"
            exit 1
          fi
          if [ "$parent_run_id" = "$child_run_id" ]; then
            echo "ERROR: nested child reused parent run id"
            cat "$nested_dir/child.out"
            exit 1
          fi
          if [ ! -f "$child_summary_file" ]; then
            echo "ERROR: missing child summary file"
            cat "$nested_dir/child.out"
            exit 1
          fi
          echo "OK: nested child run id isolated parent=$parent_run_id child=$child_run_id"
        '';
      };
    };

    workflows = model.workflows // {
      "workflow.test.nested.child" = mkWorkflow {
        id = "workflow.test.nested.child";
        mainTask = "task.test.nested.child";
      };
    };
  };

  harness = import ./lib/harness.nix {
    inherit
      pkgs
      registry
      ;
    model = nestedModel;
    inherit
      services
      serviceDefinitions
      ;
    projectRoot = ../..;
  };
in
pkgs.runCommand "isolation-nested-run-id-smoke" { } ''
  set -euo pipefail
  ${harness.shellPrelude}

  ORCH="${harness.orchestrator}/bin/nixfied-orchestrator"
  export REGISTRY_ROOT="$TMPDIR/registry"
  mkdir -p "$REGISTRY_ROOT"

  "$ORCH" run-task task.test.nested.launcher --run-id-file "$TMPDIR/top.run-id" > "$TMPDIR/top.out" 2>&1

  top_run_id="$(read_trimmed_file "$TMPDIR/top.run-id")"
  child_run_id_file="$(find "$REGISTRY_ROOT" -type f -path '*/nested-run-id/child.run-id' | head -n 1 || true)"
  child_summary_file="$(find "$REGISTRY_ROOT" -type f -path '*/nested-run-id/child.summary.json' | head -n 1 || true)"
  require_non_empty "$top_run_id" "top_run_id"
  require_non_empty "$child_run_id_file" "child_run_id_file"
  require_non_empty "$child_summary_file" "child_summary_file"
  child_run_id="$(read_trimmed_file "$child_run_id_file")"
  require_non_empty "$child_run_id" "child_run_id"
  if [ "$top_run_id" = "$child_run_id" ]; then
    fail "nested child should not reuse parent orchestrator run id"
  fi

  require_contains "$TMPDIR/top.out" "OK: nested child run id isolated"
  require_file "$child_summary_file"

  echo "OK: nested executor workflow runs get isolated run ids" > "$out"
''
