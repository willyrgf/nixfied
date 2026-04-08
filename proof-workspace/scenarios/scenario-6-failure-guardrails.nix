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
        return 1
      fi
      return 0
    }

    run_public_expect_failure() {
      local out_file="$1"
      shift
      set +e
      (
        cd "$workspace"
        HOME="$proof_home" XDG_CACHE_HOME="$proof_home/.cache" \
          REGISTRY_ROOT="$REGISTRY_ROOT" CI_ARTIFACTS_ROOT="$CI_ARTIFACTS_ROOT" \
          "$@" > "$out_file" 2>&1
      )
      local rc="$?"
      set -e
      if [ "$rc" -eq 0 ]; then
        cat "$out_file" 2>/dev/null || true
        proof_fail "expected public command to fail: $*"
      fi
      printf '%s' "$rc"
    }

    extract_last_json_line() {
      local input_file="$1"
      local output_file="$2"
      local json_line=""
      json_line="$(${pkgs.gnused}/bin/sed -n '/^{/p' "$input_file" | ${pkgs.coreutils}/bin/tail -n 1)"
      proof_require_non_empty "$json_line" "json line from $input_file"
      printf '%s\n' "$json_line" > "$output_file"
    }

    read_run_record() {
      local run_id="$1"
      local out_file="$2"
      local raw_out="$out_file.raw"
      local run_json=""
      run_public_checked "$raw_out" \
        nix run --no-write-lock-file "path:$workspace#runs" -- "$run_id"
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
      nix run --no-write-lock-file "path:$workspace#run-workflow" -- workflow.test.parallel.failfast --run-id-file "$run_id_file" --summary
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
      nix run --no-write-lock-file "path:$workspace#run-workflow" -- workflow.test.parallel.smoke --summary
    invalid_workers_rc="$?"
    set -e
    if [ "$invalid_workers_rc" -eq 0 ]; then
      cat "$TMPDIR/scenario6.invalid-workers.out" 2>/dev/null || true
      proof_fail "expected invalid worker cap invocation to fail"
    fi
    proof_require_contains "$TMPDIR/scenario6.invalid-workers.out" "ERROR:"
    proof_require_contains "$TMPDIR/scenario6.invalid-workers.out" "CI_MAX_WORKERS"

    invalid_task_rc="$(
      run_public_expect_failure "$TMPDIR/scenario6.invalid-task.out" \
        nix run --no-write-lock-file "path:$workspace#run-task" -- task.not.real
    )"
    proof_require_contains "$TMPDIR/scenario6.invalid-task.out" "ERROR:"

    invalid_workflow_rc="$(
      run_public_expect_failure "$TMPDIR/scenario6.invalid-workflow.out" \
        nix run --no-write-lock-file "path:$workspace#run-workflow" -- workflow.not.real
    )"
    proof_require_contains "$TMPDIR/scenario6.invalid-workflow.out" "ERROR:"

    runtime_owned_pass_rc="$(
      run_public_expect_failure "$TMPDIR/scenario6.runtime-owned-pass-through.out" \
        env HOME="$TMPDIR/scenario6.host-home" \
        nix run --no-write-lock-file "path:$workspace#run-task" -- task.test.runtime-owned.pass-through
    )"
    proof_require_non_empty "$runtime_owned_pass_rc" "scenario6 runtime-owned pass-through rc"
    proof_require_contains "$TMPDIR/scenario6.runtime-owned-pass-through.out" "ERROR: runtime-owned passthrough env blocked name=HOME"

    runtime_owned_env_rc="$(
      run_public_expect_failure "$TMPDIR/scenario6.runtime-owned-env.out" \
        nix run --no-write-lock-file "path:$workspace#run-task" -- task.test.runtime-owned.env
    )"
    proof_require_non_empty "$runtime_owned_env_rc" "scenario6 runtime-owned env override rc"
    proof_require_contains "$TMPDIR/scenario6.runtime-owned-env.out" "ERROR: runtime-owned env override blocked name=HOME"

    runtime_owned_scope_rc="$(
      run_public_expect_failure "$TMPDIR/scenario6.runtime-owned-scope-pass-through.out" \
        nix run --no-write-lock-file "path:$workspace#run-task" -- task.test.runtime-owned.scope-pass-through
    )"
    proof_require_non_empty "$runtime_owned_scope_rc" "scenario6 runtime-owned scope pass-through rc"
    proof_require_contains "$TMPDIR/scenario6.runtime-owned-scope-pass-through.out" "ERROR: runtime-owned passthrough env blocked name=NIXFIED_RUNTIME_DIR_SCOPE_OVERRIDE"

    sensitive_blocked_rc="$(
      run_public_expect_failure "$TMPDIR/scenario6.sensitive-blocked.out" \
        env API_KEY=top-secret \
        nix run --no-write-lock-file "path:$workspace#run-task" -- task.test.sensitive.blocked
    )"
    proof_require_non_empty "$sensitive_blocked_rc" "scenario6 sensitive blocked rc"
    proof_require_contains "$TMPDIR/scenario6.sensitive-blocked.out" "ERROR: sensitive passthrough env blocked name=API_KEY"

    run_public_checked "$TMPDIR/scenario6.sensitive-allowed.out" \
      env API_KEY=top-secret \
      nix run --no-write-lock-file "path:$workspace#run-task" -- task.test.sensitive.allowed
    proof_require_contains "$TMPDIR/scenario6.sensitive-allowed.out" "OK: allowed task received API_KEY"

    machine_output_invalid_rc="$(
      run_public_expect_failure "$TMPDIR/scenario6.machine-output-invalid.out" \
        nix run --no-write-lock-file "path:$workspace#machine-json-invalid-payload"
    )"
    proof_require_non_empty "$machine_output_invalid_rc" "scenario6 machine-output invalid rc"
    extract_last_json_line \
      "$TMPDIR/scenario6.machine-output-invalid.out" \
      "$TMPDIR/scenario6.machine-output-invalid.json"
    ${pkgs.jq}/bin/jq -e '
      .ok == false
      and .appId == "machine-json-invalid-payload"
      and .targetAppId == "json-body-invalid"
      and .failedAppId == "json-body-invalid"
      and .stage == "validation"
      and .code == "machine-output-validation-failed"
      and .contractRef == "machineOutput.result"
      and .validator == "nixfied-kernel"
    ' "$TMPDIR/scenario6.machine-output-invalid.json" >/dev/null

    install_missing_target_rc="$(
      run_public_expect_failure "$TMPDIR/scenario6.install-missing-target.out" \
        nix run --no-write-lock-file "path:$workspace#framework::install" -- --target
    )"
    proof_require_non_empty "$install_missing_target_rc" "scenario6 install missing target rc"
    proof_require_contains "$TMPDIR/scenario6.install-missing-target.out" "ERROR: --target requires a value"

    upgrade_workspace_root="$TMPDIR/scenario6.workspace-root"
    mkdir -p "$upgrade_workspace_root"
    : > "$upgrade_workspace_root/.workspace"
    upgrade_workspace_rc="$(
      run_public_expect_failure "$TMPDIR/scenario6.upgrade-workspace-root.out" \
        nix run --no-write-lock-file "path:$workspace#framework::upgrade" -- --target "$upgrade_workspace_root"
    )"
    if [ "$upgrade_workspace_rc" -ne 2 ]; then
      cat "$TMPDIR/scenario6.upgrade-workspace-root.out" 2>/dev/null || true
      proof_fail "framework::upgrade workspace-root refusal must exit with usage code 2 (got $upgrade_workspace_rc)"
    fi
    proof_require_contains "$TMPDIR/scenario6.upgrade-workspace-root.out" "ERROR: framework::upgrade refuses to target a framework workspace root: $upgrade_workspace_root"
    if [ -e "$upgrade_workspace_root/flake.nix" ]; then
      proof_fail "framework::upgrade workspace-root refusal must not create flake.nix"
    fi
    if [ -e "$upgrade_workspace_root/nixfied/VENDORED.txt" ]; then
      proof_fail "framework::upgrade workspace-root refusal must not create vendored metadata"
    fi

    echo "OK: proof workspace scenario 6 failure/guardrails passed" > "$out"
  ''
