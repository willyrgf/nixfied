{
  pkgs,
  model,
  services,
  serviceDefinitions,
  registry,
}:
let
  runtimeFixture = import ./lib/runtime-fixture.nix { inherit pkgs; };
  runtimeDeps = runtimeFixture.runtimeMaterialization {
    inherit
      pkgs
      model
      services
      serviceDefinitions
      ;
  };
  executor = import ../../nixfied/framework/runtime/executor.nix {
    inherit
      pkgs
      model
      services
      registry
      ;
    projectRoot = ../..;
    serviceDispatcherProgram = runtimeDeps.serviceDispatcherProgram;
    runtimeBin = runtimeDeps.serviceDispatcherProgram;
  };
in
pkgs.runCommand "parallel-runner-smoke" { } ''
  set -euo pipefail

  EXECUTOR="${executor}/bin/nixfied-executor"
  EVENTS_FILE="$TMPDIR/registry/events.ndjson"
  export REGISTRY_ROOT="$TMPDIR/registry"
  export CI_ARTIFACTS_ROOT="$TMPDIR/artifacts"
  export NIXFIED_RUNTIME_DIR_SCOPE_OVERRIDE="$TMPDIR/runtime-scope"
  mkdir -p "$REGISTRY_ROOT"
  mkdir -p "$CI_ARTIFACTS_ROOT"
  mkdir -p "$NIXFIED_RUNTIME_DIR_SCOPE_OVERRIDE"

  export NIXFIED_WORKFLOW_PARALLEL=1
  export NIXFIED_PARALLEL_SMOKE=1

  smoke_run_id_file="$TMPDIR/smoke.run-id"
  "$EXECUTOR" run-workflow workflow.test.parallel.smoke --run-id-file "$smoke_run_id_file" --summary > "$TMPDIR/smoke.out" 2>&1
  smoke_run_id="$(${pkgs.coreutils}/bin/tr -d '\n' < "$smoke_run_id_file")"
  if [ -z "$smoke_run_id" ]; then
    echo "missing smoke run id"
    cat "$TMPDIR/smoke.out"
    exit 1
  fi

  seq_of() {
    local run_id="$1"
    local task_id="$2"
    local state="$3"
    ${pkgs.jq}/bin/jq -r --arg runId "$run_id" --arg taskId "$task_id" --arg state "$state" '
      select((.payload.runId // "") == $runId and (.payload.taskId // "") == $taskId and (.payload.state // "") == $state) | .payload.seq
    ' "$EVENTS_FILE" | ${pkgs.coreutils}/bin/head -n 1
  }

  require_non_empty() {
    local value="$1"
    local label="$2"
    if [ -z "$value" ] || [ "$value" = "null" ]; then
      echo "missing value for $label"
      exit 1
    fi
  }

  a_running="$(seq_of "$smoke_run_id" "task.test.parallel.sleep-a" "running")"
  b_running="$(seq_of "$smoke_run_id" "task.test.parallel.sleep-b" "running")"
  c_running="$(seq_of "$smoke_run_id" "task.test.parallel.sleep-c" "running")"
  d_running="$(seq_of "$smoke_run_id" "task.test.parallel.sleep-d" "running")"
  a_passed="$(seq_of "$smoke_run_id" "task.test.parallel.sleep-a" "passed")"
  b_passed="$(seq_of "$smoke_run_id" "task.test.parallel.sleep-b" "passed")"

  require_non_empty "$a_running" "a_running"
  require_non_empty "$b_running" "b_running"
  require_non_empty "$c_running" "c_running"
  require_non_empty "$d_running" "d_running"
  require_non_empty "$a_passed" "a_passed"
  require_non_empty "$b_passed" "b_passed"

  if ! [ "$a_running" -lt "$a_passed" ] || ! [ "$b_running" -lt "$a_passed" ]; then
    echo "expected units a and b to run before unit a passed"
    exit 1
  fi

  if ! [ "$c_running" -gt "$a_passed" ]; then
    echo "expected unit c to start after unit a passed"
    exit 1
  fi

  if ! [ "$d_running" -gt "$b_passed" ]; then
    echo "expected unit d to start after unit b passed due to lock"
    exit 1
  fi

  skip_reason="$(${pkgs.jq}/bin/jq -r --arg runId "$smoke_run_id" '
    select((.payload.runId // "") == $runId and (.payload.taskId // "") == "task.test.parallel.skip" and (.payload.state // "") == "canceled") | .payload.detail.reason
  ' "$EVENTS_FILE" | ${pkgs.coreutils}/bin/head -n 1)"
  if [ "$skip_reason" != "when-false" ]; then
    echo "expected skip task to be canceled with when-false reason"
    exit 1
  fi

  max_running="$(${pkgs.jq}/bin/jq -s -r --arg runId "$smoke_run_id" '
    map(select((.payload.runId // "") == $runId and (.payload.taskId // "") != "")) |
    sort_by((.payload.seq // 0)) |
    reduce .[] as $event ({running: 0, max: 0};
      if ($event.payload.state // "") == "running" then
        .running += 1 |
        .max = (if .running > .max then .running else .max end)
      elif (($event.payload.state // "") == "passed" or ($event.payload.state // "") == "failed" or ($event.payload.state // "") == "canceled") then
        .running = (if .running > 0 then .running - 1 else 0 end)
      else
        .
      end
    ) | .max
  ' "$EVENTS_FILE")"
  if ! [ "$max_running" -le 2 ]; then
    echo "expected max running tasks <= 2, got $max_running"
    exit 1
  fi

  set +e
  failfast_run_id_file="$TMPDIR/failfast.run-id"
  "$EXECUTOR" run-workflow workflow.test.parallel.failfast --run-id-file "$failfast_run_id_file" --summary > "$TMPDIR/failfast.out" 2>&1
  failfast_rc="$?"
  set -e
  if [ "$failfast_rc" -eq 0 ]; then
    echo "expected failfast workflow to fail"
    cat "$TMPDIR/failfast.out"
    exit 1
  fi

  failfast_run_id="$(${pkgs.coreutils}/bin/tr -d '\n' < "$failfast_run_id_file")"
  if [ -z "$failfast_run_id" ]; then
    echo "missing failfast run id"
    cat "$TMPDIR/failfast.out"
    exit 1
  fi

  fail_state="$(seq_of "$failfast_run_id" "task.test.parallel.fail" "failed")"
  require_non_empty "$fail_state" "fail_state"

  slow_a_reason="$(${pkgs.jq}/bin/jq -r --arg runId "$failfast_run_id" '
    select((.payload.runId // "") == $runId and (.payload.taskId // "") == "task.test.parallel.slow-a" and (.payload.state // "") == "canceled") | .payload.detail.reason
  ' "$EVENTS_FILE" | ${pkgs.coreutils}/bin/head -n 1)"
  slow_b_reason="$(${pkgs.jq}/bin/jq -r --arg runId "$failfast_run_id" '
    select((.payload.runId // "") == $runId and (.payload.taskId // "") == "task.test.parallel.slow-b" and (.payload.state // "") == "canceled") | .payload.detail.reason
  ' "$EVENTS_FILE" | ${pkgs.coreutils}/bin/head -n 1)"
  after_reason="$(${pkgs.jq}/bin/jq -r --arg runId "$failfast_run_id" '
    select((.payload.runId // "") == $runId and (.payload.taskId // "") == "task.test.parallel.sleep-c" and (.payload.state // "") == "canceled") | .payload.detail.reason
  ' "$EVENTS_FILE" | ${pkgs.coreutils}/bin/head -n 1)"

  if [ "$slow_a_reason" != "fail-fast-running" ]; then
    echo "expected slow-a to be canceled as fail-fast-running"
    exit 1
  fi

  if [ "$slow_b_reason" != "fail-fast-running" ]; then
    echo "expected slow-b to be canceled as fail-fast-running"
    exit 1
  fi

  if [ "$after_reason" != "fail-fast" ]; then
    echo "expected dependent after unit to be canceled with fail-fast reason"
    exit 1
  fi

  after_running_count="$(${pkgs.jq}/bin/jq -r --arg runId "$failfast_run_id" '
    select((.payload.runId // "") == $runId and (.payload.taskId // "") == "task.test.parallel.sleep-c" and (.payload.state // "") == "running") | .payload.seq
  ' "$EVENTS_FILE" | ${pkgs.gnugrep}/bin/grep -c '^[0-9]' || true)"
  if [ "$after_running_count" -ne 0 ]; then
    echo "expected dependent after unit to never enter running state"
    exit 1
  fi

  echo "OK: parallel workflow scheduler behavior is validated" > "$out"
''
