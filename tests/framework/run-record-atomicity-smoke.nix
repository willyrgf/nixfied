{
  pkgs,
  model,
  registry,
}:
let
  orchestratorSource = builtins.readFile ../../nixfied/runner/orchestrator.nix;
  baseTask = model.tasks."task.ci.quality";

  probeTaskId = "task.test.run-record.atomicity";

  probeTask = baseTask // {
    id = probeTaskId;
    summary = "run record atomicity probe";
    description = "Blocks on a release gate so concurrent readers can sample orchestrator run records.";
    runner = {
      type = "shell";
      command = ''
        set -euo pipefail
        if [ -z "''${ATOMICITY_READY_DIR:-}" ] || [ -z "''${ATOMICITY_GATE_DIR:-}" ]; then
          echo "ERROR: atomicity gate env is missing"
          exit 3
        fi
        mkdir -p "$ATOMICITY_READY_DIR" "$ATOMICITY_GATE_DIR"
        : > "$ATOMICITY_READY_DIR/$NIXFIED_RUN_ID"
        while [ ! -f "$ATOMICITY_GATE_DIR/release" ]; do
          sleep 0.1
        done
        echo "OK: atomicity probe complete"
      '';
      package = null;
      workflowId = null;
    };
    runtime = baseTask.runtime // {
      passThroughEnv = (baseTask.runtime.passThroughEnv or [ ]) ++ [
        "ATOMICITY_READY_DIR"
        "ATOMICITY_GATE_DIR"
      ];
    };
    ui = baseTask.ui // {
      app = baseTask.ui.app // {
        expose = false;
        name = "test-run-record-atomicity";
      };
    };
  };

  probeModel = model // {
    tasks = model.tasks // {
      ${probeTaskId} = probeTask;
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
assert pkgs.lib.hasInfix "tmp=\"$(mktemp \"$run_file.tmp.XXXXXX\")\"" orchestratorSource;
assert pkgs.lib.hasInfix "mv \"$tmp\" \"$run_file\"" orchestratorSource;
pkgs.runCommand "run-record-atomicity-smoke" { } ''
  set -euo pipefail
  ${harness.shellPrelude}

  ORCH="${harness.orchestrator}/bin/nixfied-orchestrator"
  export REGISTRY_ROOT="$TMPDIR/registry"
  export CI_ARTIFACTS_ROOT="$TMPDIR/artifacts"
  export ATOMICITY_READY_DIR="$TMPDIR/atomicity-ready"
  export ATOMICITY_GATE_DIR="$TMPDIR/atomicity-gate"
  mkdir -p "$REGISTRY_ROOT" "$CI_ARTIFACTS_ROOT" "$ATOMICITY_READY_DIR" "$ATOMICITY_GATE_DIR"

  watch_run_records() {
    while [ ! -f "$TMPDIR/atomicity.done" ]; do
      if [ -d "$REGISTRY_ROOT/orchestrator/runs" ]; then
        while IFS= read -r run_file; do
          if ! ${pkgs.jq}/bin/jq -e '.' "$run_file" > /dev/null 2>&1; then
            printf '%s\n' "$run_file" > "$TMPDIR/atomicity.err"
            return 1
          fi
        done < <(${pkgs.findutils}/bin/find "$REGISTRY_ROOT/orchestrator/runs" -type f -name '*.json' | ${pkgs.coreutils}/bin/sort)
      fi
    done
  }

  ready_count_is() {
    local expected="$1"
    local count

    count="$(${pkgs.findutils}/bin/find "$ATOMICITY_READY_DIR" -type f | ${pkgs.coreutils}/bin/wc -l | ${pkgs.coreutils}/bin/tr -d '[:space:]')"
    [ "$count" = "$expected" ]
  }

  watch_run_records > "$TMPDIR/atomicity-watch.out" 2>&1 &
  watch_pid="$!"

  set +e
  pids=()
  run_index=1
  while [ "$run_index" -le 16 ]; do
    "$ORCH" run-task "${probeTaskId}" > "$TMPDIR/run-$run_index.out" 2>&1 &
    pids+=("$!")
    run_index="$((run_index + 1))"
  done
  set -e

  wait_for_condition 60 "all atomicity runs to block on the gate" ready_count_is 16

  : > "$ATOMICITY_GATE_DIR/release"

  failed_runs=0
  for pid in "''${pids[@]}"; do
    wait "$pid" || failed_runs="$((failed_runs + 1))"
  done

  touch "$TMPDIR/atomicity.done"
  wait "$watch_pid" || true

  if [ "$failed_runs" -ne 0 ]; then
    fail "expected atomicity probe runs to pass"
  fi

  if [ -f "$TMPDIR/atomicity.err" ]; then
    echo "reader observed invalid run record: $(cat "$TMPDIR/atomicity.err")"
    exit 1
  fi

  echo "OK: run records stay parseable during concurrent creation" > "$out"
''
