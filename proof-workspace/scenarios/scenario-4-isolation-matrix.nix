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
  baseIsolationProbeWorkflow = model.workflows."workflow.test.isolation.probe";

  scenarioUnitTaskId = "task.test.proof.scenario4.unit";
  scenarioWorkflowId = "workflow.test.proof.scenario4.probe";

  scenarioUnitTask =
    baseTask
    // {
      id = scenarioUnitTaskId;
      summary = "proof scenario 4 isolation matrix probe unit";
      description = "proof scenario 4 isolation matrix probe unit";
      runner = {
        type = "shell";
        package = null;
        workflowId = null;
        command = ''
          set -euo pipefail
          artifacts_dir="''${CI_ARTIFACTS_DIR:-$REGISTRY_ROOT/artifacts/manual}"
          mkdir -p "$artifacts_dir"
          {
            printf 'slot=%s\n' "''${NIX_ENV:-}"
            printf 'env=%s\n' "''${PROJECT_ENV:-}"
            printf 'registry_root=%s\n' "''${REGISTRY_ROOT:-}"
            printf 'artifacts_dir=%s\n' "''${CI_ARTIFACTS_DIR:-}"
          } > "$artifacts_dir/proof-isolation-probe.txt"
          echo "OK: proof isolation probe complete slot=''${NIX_ENV:-} env=''${PROJECT_ENV:-}"
        '';
      };
      runtime = (baseTask.runtime or { }) // {
        preHooks = { };
        postHooks = { };
      };
    };

  scenarioProbeUnit = (baseIsolationProbeWorkflow.units.probe or { }) // {
    taskId = scenarioUnitTaskId;
  };

  scenarioProbeWorkflow =
    baseIsolationProbeWorkflow
    // {
      id = scenarioWorkflowId;
      summary = "proof scenario 4 isolation matrix probe workflow";
      description = "proof scenario 4 isolation matrix probe workflow";
      units = (baseIsolationProbeWorkflow.units or { }) // {
        probe = scenarioProbeUnit;
      };
      plan = [
        (
          {
            name = "probe";
          }
          // scenarioProbeUnit
        )
      ];
    };

  scenarioModel = model // {
    tasks = model.tasks // {
      "${scenarioUnitTaskId}" = scenarioUnitTask;
    };
    workflows = model.workflows // {
      "${scenarioWorkflowId}" = scenarioProbeWorkflow;
    };
  };

  harness = import ../../tests/framework/lib/harness.nix {
    inherit
      pkgs
      services
      serviceDefinitions
      registry
      ;
    model = scenarioModel;
    projectRoot = ../..;
  };
in
pkgs.runCommand "proof-workspace-scenario-4-isolation-matrix"
  {
    nativeBuildInputs = [
      pkgs.coreutils
      pkgs.findutils
      pkgs.git
      pkgs.gnugrep
      pkgs.gnused
    ];
  }
  ''
    set -euo pipefail
    ${materialize.shellPrelude}

    ORCH="${harness.orchestrator}/bin/nixfied-orchestrator"
    proof_require_file "$ORCH"

    export REGISTRY_ROOT="$TMPDIR/registry"
    export CI_ARTIFACTS_ROOT="$TMPDIR/artifacts"
    mkdir -p "$REGISTRY_ROOT" "$CI_ARTIFACTS_ROOT"

    workspace="$TMPDIR/proof-seed"
    proof_workspace_bootstrap_seed_copy "$workspace"

    logs_root="$TMPDIR/isolation-logs"
    mkdir -p "$logs_root"

    cell_count=0
    for slot_value in 5 7; do
      for env_value in dev test prod; do
        cell_count=$((cell_count + 1))
        cell_name="slot-''${slot_value}__env-''${env_value}"
        cell_dir="$logs_root/$cell_name"
        registry_dir="$cell_dir/registry"
        artifacts_dir="$cell_dir/artifacts"
        mkdir -p "$registry_dir" "$artifacts_dir"

        if ! NIX_ENV="$slot_value" PROJECT_ENV="$env_value" \
          REGISTRY_ROOT="$registry_dir" CI_ARTIFACTS_ROOT="$artifacts_dir" \
          NIXFIED_CALLER_PWD="$workspace" \
          "$ORCH" run-workflow ${scenarioWorkflowId} --summary > "$cell_dir/run.log" 2>&1; then
          cat "$cell_dir/run.log" 2>/dev/null || true
          exit 1
        fi

        proof_require_file "$cell_dir/run.log"
        proof_require_contains "$cell_dir/run.log" "OK: proof isolation probe complete slot=$slot_value env=$env_value"
        proof_require_file "$registry_dir/events.ndjson"

        summary_json="$(${pkgs.gnused}/bin/sed -n 's/^INFO: summary_json=//p' "$cell_dir/run.log" | ${pkgs.coreutils}/bin/tail -n 1)"
        proof_require_non_empty "$summary_json" "summary_json for $cell_name"
        proof_require_file "$summary_json"
        case "$summary_json" in
          "$artifacts_dir"/*) ;;
          *)
            proof_fail "summary path escaped cell artifacts root for $cell_name: $summary_json"
            ;;
        esac

      done
    done

    unique_cell_count="$(${pkgs.findutils}/bin/find "$logs_root" -mindepth 1 -maxdepth 1 -type d -name 'slot-*__env-*' | ${pkgs.gnused}/bin/sed 's|.*/||' | ${pkgs.coreutils}/bin/sort | ${pkgs.coreutils}/bin/uniq | ${pkgs.coreutils}/bin/wc -l | tr -d ' ')"
    if [ "$unique_cell_count" -ne "$cell_count" ]; then
      proof_fail "isolation cells are not unique (cells=$cell_count unique=$unique_cell_count)"
    fi

    echo "OK: proof workspace scenario 4 isolation matrix passed" > "$out"
  ''
