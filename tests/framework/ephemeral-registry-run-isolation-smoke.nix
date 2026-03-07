{
  pkgs,
  model,
  registry,
}:
let
  baseTask = model.tasks."task.ci.quality";

  probeTaskId = "task.test.ephemeral.registry-isolation";
  probeWorkflowId = "workflow.test.ephemeral.registry-isolation";

  probeTask = baseTask // {
    id = probeTaskId;
    summary = "ephemeral registry isolation probe";
    description = "Fails intentionally so each ephemeral root can be inspected after concurrent runs.";
    runner = {
      type = "shell";
      command = ''
        set -euo pipefail
        printf '%s\n' "$REGISTRY_ROOT" > "$CI_ARTIFACTS_DIR/registry-root.txt"
        echo "INFO: probe_registry=$REGISTRY_ROOT"
        if [ -n "''${NIXFIED_EPHEMERAL_GATE_DIR:-}" ]; then
          ready_file="$NIXFIED_EPHEMERAL_GATE_DIR/$NIXFIED_RUN_ID.ready"
          release_file="$NIXFIED_EPHEMERAL_GATE_DIR/release"
          mkdir -p "$NIXFIED_EPHEMERAL_GATE_DIR"
          : > "$ready_file"
          while [ ! -f "$release_file" ]; do
            sleep 0.1
          done
        fi
        echo "ERROR: intentional failure to preserve ephemeral state" >&2
        exit 1
      '';
      package = null;
      workflowId = null;
    };
    runtime = baseTask.runtime // {
      passThroughEnv = (baseTask.runtime.passThroughEnv or [ ]) ++ [ "NIXFIED_EPHEMERAL_GATE_DIR" ];
    };
    ui = baseTask.ui // {
      app = baseTask.ui.app // {
        expose = false;
        name = "test-ephemeral-registry-isolation";
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
    summary = "ephemeral registry isolation probe workflow";
    description = "Validates run-local registry roots for concurrent ephemeral runs.";
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
      ephemeral = model.runtime.ephemeral // {
        keepFailures = true;
        maxFailedRoots = 16;
      };
    };
    tasks = model.tasks // {
      ${probeTaskId} = probeTask;
    };
    workflows = model.workflows // {
      ${probeWorkflowId} = probeWorkflow;
    };
  };

  harness = import ./lib/harness.nix {
    inherit
      pkgs
      registry
      ;
    model = probeModel;
    projectRoot = ../..;
  };
in
pkgs.runCommand "ephemeral-registry-run-isolation-smoke" { } ''
  set -euo pipefail
  ${harness.shellPrelude}

  ORCH="${harness.orchestrator}/bin/nixfied-orchestrator"
  export REGISTRY_ROOT="$TMPDIR/host-registry"
  export NIXFIED_EPHEMERAL_ROOT_BASE="$TMPDIR/ephemeral-roots"
  gate_dir="$TMPDIR/gates"
  mkdir -p "$REGISTRY_ROOT" "$NIXFIED_EPHEMERAL_ROOT_BASE" "$gate_dir"

  set +e
  NIXFIED_EPHEMERAL_GATE_DIR="$gate_dir" "$ORCH" run-workflow "${probeWorkflowId}" --run-id-file "$TMPDIR/run-1.run-id" --summary > "$TMPDIR/run-1.out" 2>&1 &
  pid_one="$!"
  NIXFIED_EPHEMERAL_GATE_DIR="$gate_dir" "$ORCH" run-workflow "${probeWorkflowId}" --run-id-file "$TMPDIR/run-2.run-id" --summary > "$TMPDIR/run-2.out" 2>&1 &
  pid_two="$!"
  wait_for_condition 30 "run one id" test -s "$TMPDIR/run-1.run-id"
  wait_for_condition 30 "run two id" test -s "$TMPDIR/run-2.run-id"
  run_one="$(read_trimmed_file "$TMPDIR/run-1.run-id")"
  run_two="$(read_trimmed_file "$TMPDIR/run-2.run-id")"
  wait_for_condition 60 "run one ready" test -f "$gate_dir/$run_one.ready"
  wait_for_condition 60 "run two ready" test -f "$gate_dir/$run_two.ready"
  : > "$gate_dir/release"
  wait "$pid_one"
  rc_one="$?"
  wait "$pid_two"
  rc_two="$?"
  set -e

  if [ "$rc_one" -eq 0 ] || [ "$rc_two" -eq 0 ]; then
    fail "expected probe workflows to fail so ephemeral roots are preserved"
  fi

  root_one="$(${pkgs.gnused}/bin/sed -n 's/^INFO: Root: //p' "$TMPDIR/run-1.out" | ${pkgs.coreutils}/bin/tail -n 1)"
  root_two="$(${pkgs.gnused}/bin/sed -n 's/^INFO: Root: //p' "$TMPDIR/run-2.out" | ${pkgs.coreutils}/bin/tail -n 1)"
  require_non_empty "$run_one" "run_one"
  require_non_empty "$run_two" "run_two"
  require_non_empty "$root_one" "root_one"
  require_non_empty "$root_two" "root_two"

  if [ "$root_one" = "$root_two" ]; then
    fail "expected concurrent ephemeral runs to preserve distinct roots"
  fi

  if [ -e "$root_one" ] || [ -e "$root_two" ]; then
    fail "expected failed ephemeral roots to be renamed out of their original locations"
  fi

  check_root() {
    local root="$1"
    local events_file="$root/registry/events.ndjson"
    local summary_file="$root/artifacts/summary.json"
    local registry_file="$root/artifacts/registry-root.txt"
    local registry_value
    local expected_registry
    local unique_run_count
    local only_run_id

    require_file "$events_file"
    require_file "$summary_file"
    require_file "$registry_file"

    unique_run_count="$(${pkgs.jq}/bin/jq -r '.runId' "$events_file" | ${pkgs.coreutils}/bin/sort -u | ${pkgs.coreutils}/bin/wc -l | ${pkgs.coreutils}/bin/tr -d '[:space:]')"
    only_run_id="$(${pkgs.jq}/bin/jq -r '.runId' "$events_file" | ${pkgs.coreutils}/bin/sort -u | ${pkgs.coreutils}/bin/head -n 1)"
    if [ "$unique_run_count" != "1" ] || [ -z "$only_run_id" ]; then
      fail "expected preserved events to contain exactly one run id for $root"
    fi

    registry_value="$(${pkgs.coreutils}/bin/cat "$registry_file")"
    case "$only_run_id" in
      "$run_one")
        expected_registry="$root_one/registry"
        ;;
      "$run_two")
        expected_registry="$root_two/registry"
        ;;
      *)
        fail "unexpected run id recorded in preserved root: $only_run_id"
        ;;
    esac

    if [ "$registry_value" != "$expected_registry" ]; then
      fail "registry root artifact did not match the original run-local registry for $only_run_id"
    fi

    ${pkgs.jq}/bin/jq -e --arg run "$only_run_id" '.run_id == $run' "$summary_file" > /dev/null
    printf '%s' "$only_run_id"
  }

  mapfile -t preserved_roots < <(
    ${pkgs.findutils}/bin/find "$NIXFIED_EPHEMERAL_ROOT_BASE" -mindepth 1 -maxdepth 1 -type d -name "${model.identity.projectId}-ephemeral-failed-*" |
      ${pkgs.coreutils}/bin/sort
  )
  if [ "''${#preserved_roots[@]}" != "2" ]; then
    fail "expected exactly two preserved ephemeral roots"
  fi

  observed_runs="$(
    for root in "''${preserved_roots[@]}"; do
      check_root "$root"
      printf '\n'
    done | ${pkgs.coreutils}/bin/sort
  )"
  expected_runs="$(
    printf '%s\n%s\n' "$run_one" "$run_two" | ${pkgs.coreutils}/bin/sort
  )"

  if [ "$observed_runs" != "$expected_runs" ]; then
    echo "observed preserved runs:"
    printf '%s\n' "$observed_runs"
    echo "expected runs:"
    printf '%s\n' "$expected_runs"
    fail "preserved ephemeral roots did not match the expected run ids"
  fi

  echo "OK: concurrent ephemeral runs keep isolated registry roots" > "$out"
''
