{ pkgs, ... }:
let
  materialize = import ../lib/materialize.nix {
    inherit pkgs;
    repoRoot = ../..;
  };
in
pkgs.runCommand "proof-workspace-scenario-6-failure-guardrails"
  {
    nativeBuildInputs = [
      pkgs.coreutils
      pkgs.git
      pkgs.gnugrep
      pkgs.gnused
      pkgs.jq
      pkgs.nix
      pkgs.procps
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

    proof_home="$workspace/.proof-home"
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
        return 1
      fi
      return 0
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

    run_id_file="$TMPDIR/scenario6.failfast.run-id"
    child_leak_dir="$TMPDIR/scenario6.failfast.child"
    mkdir -p "$child_leak_dir"

    set +e
    run_public_checked "$TMPDIR/scenario6.failfast.out" \
      env \
      NIXFIED_WORKFLOW_PARALLEL=1 \
      NIXFIED_PARALLEL_SMOKE=1 \
      NIXFIED_PARALLEL_CHILD_LEAK_DIR="$child_leak_dir" \
      nix run "path:$workspace#run-workflow" -- workflow.test.parallel.failfast --run-id-file "$run_id_file" --summary
    failfast_rc="$?"
    set -e
    if [ "$failfast_rc" -eq 0 ]; then
      cat "$TMPDIR/scenario6.failfast.out" 2>/dev/null || true
      proof_fail "expected failfast workflow to fail"
    fi

    proof_require_file "$run_id_file"
    failfast_run_id="$(tr -d '\n' < "$run_id_file")"
    proof_require_non_empty "$failfast_run_id" "scenario6 failfast run id"
    read_run_record "$failfast_run_id" "$TMPDIR/scenario6.failfast.record.json"
    ${pkgs.jq}/bin/jq -e '
      .payload.state == "failed"
      and .payload.command == "run-workflow"
      and .payload.workflow_id == "workflow.test.parallel.failfast"
      and ((.payload.exit_code | type) == "number")
      and (.payload.exit_code == 7)
      and (.payload.run_id == $runId)
      and (.payload | has("attempt_id"))
      and (.payload | has("history"))
    ' --arg runId "$failfast_run_id" "$TMPDIR/scenario6.failfast.record.json" >/dev/null
    proof_require_contains "$TMPDIR/scenario6.failfast.out" "ERROR:"

    child_pid_file="$child_leak_dir/slow-a.pid"
    proof_require_file "$child_pid_file"
    child_pid="$(tr -d '\n' < "$child_pid_file")"
    proof_require_non_empty "$child_pid" "scenario6 failfast child pid"
    wait_for_pid_exit "$child_pid" 30 "scenario6 failfast child"

    set +e
    run_public_checked "$TMPDIR/scenario6.invalid-workers.out" \
      env \
      NIXFIED_WORKFLOW_PARALLEL=1 \
      NIXFIED_PARALLEL_SMOKE=1 \
      CI_MAX_WORKERS=0 \
      nix run "path:$workspace#run-workflow" -- workflow.test.parallel.smoke --summary
    invalid_workers_rc="$?"
    set -e
    if [ "$invalid_workers_rc" -eq 0 ]; then
      cat "$TMPDIR/scenario6.invalid-workers.out" 2>/dev/null || true
      proof_fail "expected invalid worker cap invocation to fail"
    fi
    proof_require_contains "$TMPDIR/scenario6.invalid-workers.out" "ERROR:"
    proof_require_contains "$TMPDIR/scenario6.invalid-workers.out" "CI_MAX_WORKERS"

    set +e
    run_public_checked "$TMPDIR/scenario6.invalid-task.out" \
      nix run "path:$workspace#run-task" -- task.not.real
    invalid_task_rc="$?"
    set -e
    if [ "$invalid_task_rc" -eq 0 ]; then
      cat "$TMPDIR/scenario6.invalid-task.out" 2>/dev/null || true
      proof_fail "expected unknown task invocation to fail"
    fi
    proof_require_contains "$TMPDIR/scenario6.invalid-task.out" "ERROR:"

    set +e
    run_public_checked "$TMPDIR/scenario6.invalid-workflow.out" \
      nix run "path:$workspace#run-workflow" -- workflow.not.real
    invalid_workflow_rc="$?"
    set -e
    if [ "$invalid_workflow_rc" -eq 0 ]; then
      cat "$TMPDIR/scenario6.invalid-workflow.out" 2>/dev/null || true
      proof_fail "expected unknown workflow invocation to fail"
    fi
    proof_require_contains "$TMPDIR/scenario6.invalid-workflow.out" "ERROR:"

    echo "OK: proof workspace scenario 6 failure/guardrails passed" > "$out"
  ''
