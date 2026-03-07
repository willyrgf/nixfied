{
  pkgs,
  model,
  registry,
}:
let
  probeModel = model // {
    workflows = model.workflows // {
      "workflow.ci.basic" = model.workflows."workflow.ci.basic" // {
        execution = model.workflows."workflow.ci.basic".execution // {
          ephemeral = (model.workflows."workflow.ci.basic".execution.ephemeral or { }) // {
            enable = false;
          };
        };
      };
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
pkgs.runCommand "summary-json-smoke" { } ''
  set -euo pipefail

  ORCH="${orchestrator}/bin/nixfied-orchestrator"
  export REGISTRY_ROOT="$TMPDIR/registry"
  export CI_ARTIFACTS_ROOT="$TMPDIR/artifacts"
  mkdir -p "$REGISTRY_ROOT" "$CI_ARTIFACTS_ROOT"

  "$ORCH" run-workflow workflow.ci.basic --summary > "$TMPDIR/ci.out" 2>&1

  run_id="$(${pkgs.gnused}/bin/sed -n 's/^INFO: runId=\([^ ]*\).*/\1/p' "$TMPDIR/ci.out" | ${pkgs.coreutils}/bin/tail -n 1)"
  if [ -z "$run_id" ]; then
    echo "missing run id"
    cat "$TMPDIR/ci.out"
    exit 1
  fi
  summary_file="$CI_ARTIFACTS_ROOT/$run_id/summary.json"
  if [ ! -f "$summary_file" ]; then
    echo "missing summary file: $summary_file"
    cat "$TMPDIR/ci.out"
    exit 1
  fi

  ${pkgs.jq}/bin/jq -e '
    .run_id
    and .workflow_id == "workflow.ci.basic"
    and .mode == "ci"
    and (.exit_code | type == "number")
    and (.started_at | type == "string")
    and (.finished_at | type == "string")
    and (.duration_seconds | type == "number")
    and (.counts.passed | type == "number")
    and (.counts.failed | type == "number")
    and (.counts.canceled | type == "number")
    and (.steps | type == "array")
    and (.steps | length >= 2)
    and ([.steps[] | .name | type] | all(. == "string"))
    and ([.steps[] | .status | type] | all(. == "string"))
    and ([.steps[] | .duration | type] | all(. == "number"))
    and (.timing.total_duration | type == "number")
    and (.timing.setup_duration | type == "number")
    and (.timing.steps_duration | type == "number")
    and (.timing.teardown_duration | type == "number")
    and (.timing.accounted_duration | type == "number")
    and (.timing.untracked_duration | type == "number")
    and ((.timing.parallelism.max_workers | type) == "number" or (.timing.parallelism.max_workers | type) == "null")
    and ((.timing.parallelism.peak_workers | type) == "number" or (.timing.parallelism.peak_workers | type) == "null")
    and ((.timing.parallelism.canceled_count | type) == "number" or (.timing.parallelism.canceled_count | type) == "null")
  ' "$summary_file" > /dev/null

  ${pkgs.gnugrep}/bin/grep -Fq "Summary" "$TMPDIR/ci.out"
  ${pkgs.gnugrep}/bin/grep -Fq "[PASS] task.ci.quality" "$TMPDIR/ci.out"
  ${pkgs.gnugrep}/bin/grep -Fq "[PASS] task.ci.tests" "$TMPDIR/ci.out"
  ${pkgs.gnugrep}/bin/grep -Fq "Total time:" "$TMPDIR/ci.out"
  ${pkgs.gnugrep}/bin/grep -Fq "INFO: Time breakdown" "$TMPDIR/ci.out"
  ${pkgs.gnugrep}/bin/grep -Fq "INFO: Parallelism" "$TMPDIR/ci.out"
  ${pkgs.gnugrep}/bin/grep -Fq "OK: Exit code: 0" "$TMPDIR/ci.out"

  echo "OK: workflow summary json contract validated" > "$out"
''
