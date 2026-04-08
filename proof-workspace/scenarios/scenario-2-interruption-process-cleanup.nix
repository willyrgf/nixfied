{ pkgs, ... }:
let
  materialize = import ../lib/materialize.nix {
    inherit pkgs;
    repoRoot = ../..;
  };
in
pkgs.runCommand "proof-workspace-scenario-2-interruption-process-cleanup"
  {
    nativeBuildInputs = [
      pkgs.coreutils
      pkgs.git
      pkgs.gnugrep
      pkgs.jq
      pkgs.nix
      pkgs.procps
      pkgs.gnused
    ];
  }
  ''
    set -euo pipefail
    ${materialize.shellPrelude}

    export REGISTRY_ROOT="$TMPDIR/registry"
    export CI_ARTIFACTS_ROOT="$TMPDIR/artifacts"
    mkdir -p "$REGISTRY_ROOT" "$CI_ARTIFACTS_ROOT"

    workspace="$TMPDIR/proof-seed"
    proof_workspace_bootstrap_seed_copy "$workspace"
    proof_require_dir "$workspace/.git"
    proof_require_file "$workspace/flake.nix"

    proof_home="$TMPDIR/proof-home"
    mkdir -p "$proof_home/.cache"

    run_public_checked() {
      local out_file="$1"
      shift
      if ! (
        cd "$workspace"
        HOME="$proof_home" XDG_CACHE_HOME="$proof_home/.cache" \
          REGISTRY_ROOT="$REGISTRY_ROOT" CI_ARTIFACTS_ROOT="$CI_ARTIFACTS_ROOT" \
          "$@" > "$out_file" 2>&1
      ); then
        cat "$out_file" 2>/dev/null || true
        proof_fail "public command failed: $*"
      fi
    }

    wait_for_file() {
      local target_file="$1"
      local timeout_seconds="$2"
      local deadline="$(( $(date +%s) + timeout_seconds ))"
      while [ ! -f "$target_file" ]; do
        if [ "$(date +%s)" -ge "$deadline" ]; then
          proof_fail "timed out waiting for file: $target_file"
        fi
        sleep 1
      done
    }

    read_run_record() {
      local run_id="$1"
      local out_file="$2"
      local raw_out="$out_file.raw"
      local run_json=""
      run_public_checked "$raw_out" \
        nix run "path:$workspace#runs" -- "$run_id"
      run_json="$(${pkgs.gnused}/bin/sed -n '/^{/p' "$raw_out" | ${pkgs.coreutils}/bin/tail -n 1)"
      proof_require_non_empty "$run_json" "run record json for $run_id"
      printf '%s\n' "$run_json" > "$out_file"
    }

    wait_for_run_state() {
      local run_id="$1"
      local expected_state="$2"
      local timeout_seconds="$3"
      local deadline="$(( $(date +%s) + timeout_seconds ))"
      local state=""
      while true; do
        read_run_record "$run_id" "$TMPDIR/runs.$run_id.json"
        state="$(${pkgs.jq}/bin/jq -r '.payload.state // ""' "$TMPDIR/runs.$run_id.json")"
        if [ "$state" = "$expected_state" ]; then
          return 0
        fi
        if [ "$(date +%s)" -ge "$deadline" ]; then
          proof_fail "timed out waiting for run state run_id=$run_id expected=$expected_state actual=$state"
        fi
        sleep 1
      done
    }

    wait_for_pid_exit() {
      local pid="$1"
      local timeout_seconds="$2"
      local label="$3"
      local deadline="$(( $(date +%s) + timeout_seconds ))"
      while ${pkgs.procps}/bin/ps -p "$pid" > /dev/null 2>&1; do
        if [ "$(date +%s)" -ge "$deadline" ]; then
          ${pkgs.procps}/bin/ps -p "$pid" -o pid=,ppid=,command= || true
          proof_fail "timed out waiting for process exit label=$label pid=$pid"
        fi
        sleep 1
      done
    }

    start_bg_slow_task() {
      local run_id_file="$1"
      local child_leak_dir="$2"
      local out_file="$3"
      mkdir -p "$child_leak_dir"
      run_public_checked "$out_file" \
        env NIXFIED_PARALLEL_CHILD_LEAK_DIR="$child_leak_dir" \
        nix run "path:$workspace#run-task" -- task.test.parallel.slow-a --run-id-file "$run_id_file" --bg
      proof_require_file "$run_id_file"
      proof_require_non_empty "$(tr -d '\n' < "$run_id_file")" "run id in $run_id_file"
    }

    run_one_id_file="$TMPDIR/scenario2.run-one.id"
    run_two_id_file="$TMPDIR/scenario2.run-two.id"
    run_three_id_file="$TMPDIR/scenario2.run-three.id"

    run_one_child_dir="$TMPDIR/scenario2.run-one.child"
    run_two_child_dir="$TMPDIR/scenario2.run-two.child"
    run_three_child_dir="$TMPDIR/scenario2.run-three.child"

    run_public_checked "$TMPDIR/scenario2.prewarm.runs.out" \
      nix run "path:$workspace#runs"
    run_public_checked "$TMPDIR/scenario2.prewarm.stop-all-runs.out" \
      nix run "path:$workspace#stop-all-runs"

    start_bg_slow_task "$run_one_id_file" "$run_one_child_dir" "$TMPDIR/scenario2.run-one.bg.out"
    run_one_id="$(tr -d '\n' < "$run_one_id_file")"
    wait_for_run_state "$run_one_id" "running" 30
    wait_for_file "$run_one_child_dir/slow-a.pid" 30
    run_one_child_pid="$(tr -d '\n' < "$run_one_child_dir/slow-a.pid")"
    proof_require_non_empty "$run_one_child_pid" "scenario2 run one child pid"

    run_public_checked "$TMPDIR/scenario2.stop-run.out" \
      nix run "path:$workspace#stop-run" -- "$run_one_id"
    wait_for_run_state "$run_one_id" "canceled" 30

    read_run_record "$run_one_id" "$TMPDIR/scenario2.run-one.record.json"
    ${pkgs.jq}/bin/jq -e '
      .payload.state == "canceled"
      and .payload.stop_reason == "stop-requested"
      and ((.payload.pid | type) == "number")
      and (.payload.pid > 0)
      and (.payload | has("pgid"))
    ' "$TMPDIR/scenario2.run-one.record.json" >/dev/null
    wait_for_pid_exit "$run_one_child_pid" 30 "scenario2 run one child"

    start_bg_slow_task "$run_two_id_file" "$run_two_child_dir" "$TMPDIR/scenario2.run-two.bg.out"
    start_bg_slow_task "$run_three_id_file" "$run_three_child_dir" "$TMPDIR/scenario2.run-three.bg.out"
    run_two_id="$(tr -d '\n' < "$run_two_id_file")"
    run_three_id="$(tr -d '\n' < "$run_three_id_file")"
    wait_for_run_state "$run_two_id" "running" 30
    wait_for_run_state "$run_three_id" "running" 30
    wait_for_file "$run_two_child_dir/slow-a.pid" 30
    wait_for_file "$run_three_child_dir/slow-a.pid" 30
    run_two_child_pid="$(tr -d '\n' < "$run_two_child_dir/slow-a.pid")"
    run_three_child_pid="$(tr -d '\n' < "$run_three_child_dir/slow-a.pid")"

    run_public_checked "$TMPDIR/scenario2.stop-all-runs.out" \
      nix run "path:$workspace#stop-all-runs"
    wait_for_run_state "$run_two_id" "canceled" 30
    wait_for_run_state "$run_three_id" "canceled" 30

    read_run_record "$run_two_id" "$TMPDIR/scenario2.run-two.record.json"
    read_run_record "$run_three_id" "$TMPDIR/scenario2.run-three.record.json"
    ${pkgs.jq}/bin/jq -e '.payload.stop_reason == "stop-requested"' "$TMPDIR/scenario2.run-two.record.json" >/dev/null
    ${pkgs.jq}/bin/jq -e '.payload.stop_reason == "stop-requested"' "$TMPDIR/scenario2.run-three.record.json" >/dev/null
    wait_for_pid_exit "$run_two_child_pid" 30 "scenario2 run two child"
    wait_for_pid_exit "$run_three_child_pid" 30 "scenario2 run three child"

    run_public_checked "$TMPDIR/scenario2.runs.list.out" \
      nix run "path:$workspace#runs"
    proof_require_contains "$TMPDIR/scenario2.runs.list.out" "$run_one_id"
    proof_require_contains "$TMPDIR/scenario2.runs.list.out" "$run_two_id"
    proof_require_contains "$TMPDIR/scenario2.runs.list.out" "$run_three_id"

    echo "OK: proof workspace scenario 2 interruption/process cleanup passed" > "$out"
  ''
