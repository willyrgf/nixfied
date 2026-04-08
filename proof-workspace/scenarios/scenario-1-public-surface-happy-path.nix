{ pkgs, ... }:
let
  materialize = import ../lib/materialize.nix {
    inherit pkgs;
    repoRoot = ../..;
  };
in
pkgs.runCommand "proof-workspace-scenario-1-public-surface-happy-path"
  {
    nativeBuildInputs = [
      pkgs.coreutils
      pkgs.diffutils
      pkgs.git
      pkgs.gnugrep
      pkgs.gnused
      pkgs.jq
      pkgs.nix
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

    read_run_record() {
      local run_id="$1"
      local out_file="$2"
      local raw_out="$out_file.raw"
      local run_json=""
      run_public_checked "$raw_out" nix run --no-write-lock-file "path:$workspace#runs" -- "$run_id"
      run_json="$(${pkgs.gnused}/bin/sed -n '/^{/p' "$raw_out" | ${pkgs.coreutils}/bin/tail -n 1)"
      proof_require_non_empty "$run_json" "run record json for $run_id"
      printf '%s\n' "$run_json" > "$out_file"
    }

    extract_last_json_line() {
      local input_file="$1"
      local output_file="$2"
      local json_line=""
      json_line="$(${pkgs.gnused}/bin/sed -n '/^{/p' "$input_file" | ${pkgs.coreutils}/bin/tail -n 1)"
      proof_require_non_empty "$json_line" "json line from $input_file"
      printf '%s\n' "$json_line" > "$output_file"
    }

    run_public_checked "$TMPDIR/scenario1.help.out" \
      nix run --no-write-lock-file "path:$workspace#help"
    run_public_checked "$TMPDIR/scenario1.features.out" \
      nix run --no-write-lock-file "path:$workspace#features"
    run_public_checked "$TMPDIR/scenario1.introspect.out" \
      nix run --no-write-lock-file "path:$workspace#introspect" -- app:validate-env --json
    run_public_checked "$TMPDIR/scenario1.validate-env.out" \
      nix run --no-write-lock-file "path:$workspace#validate-env"
    run_public_checked "$TMPDIR/scenario1.machine-output.out" \
      nix run --no-write-lock-file "path:$workspace#machine-json"
    run_public_checked "$TMPDIR/scenario1.ports.out" \
      nix run --no-write-lock-file "path:$workspace#ports"
    run_public_checked "$TMPDIR/scenario1.check-ports.out" \
      nix run --no-write-lock-file "path:$workspace#check-ports"

    proof_require_contains "$TMPDIR/scenario1.features.out" "runtime.machine-output-behavior"
    proof_require_contains "$TMPDIR/scenario1.features.out" "runtime.summary-sidecars"
    proof_require_contains "$TMPDIR/scenario1.features.out" "runtime.task-hooks"
    proof_require_contains "$TMPDIR/scenario1.features.out" "runtime.stop-run-semantics"
    proof_require_contains "$TMPDIR/scenario1.features.out" "runtime.process-group-cleanup"
    proof_require_contains "$TMPDIR/scenario1.features.out" "runtime.install-semantics"
    proof_require_contains "$TMPDIR/scenario1.features.out" "runtime.upgrade-semantics"
    proof_require_contains "$TMPDIR/scenario1.features.out" "runtime.workflow-interruption-semantics"
    proof_require_contains "$TMPDIR/scenario1.features.out" "runtime.artifact-placement-semantics"
    proof_require_contains "$TMPDIR/scenario1.validate-env.out" "OK:"
    proof_require_contains "$TMPDIR/scenario1.machine-output.out" "json-body human log"
    extract_last_json_line \
      "$TMPDIR/scenario1.machine-output.out" \
      "$TMPDIR/scenario1.machine-output.json"
    ${pkgs.jq}/bin/jq -e '.ok == true and .kind == "task"' "$TMPDIR/scenario1.machine-output.json" >/dev/null
    if ${pkgs.gnugrep}/bin/grep -Fq "json-body human log" "$TMPDIR/scenario1.machine-output.json"; then
      proof_fail "machine-output happy path must keep human log text off the JSON payload"
    fi

    task_run_id_file="$TMPDIR/scenario1.task.run-id"
    seq_run_id_file="$TMPDIR/scenario1.seq.run-id"
    par_run_id_file="$TMPDIR/scenario1.par.run-id"
    hook_log_file="$TMPDIR/scenario1.hooks.log"
    workflow_phase_log_file="$TMPDIR/scenario1.workflow-phase.log"

    for service in helios minio nginx postgres reth; do
      run_public_checked "$TMPDIR/scenario1.$service.full-start.out" \
        nix run --no-write-lock-file "path:$workspace#svc::$service::full-start"
      proof_require_contains "$TMPDIR/scenario1.$service.full-start.out" "OK:"

      run_public_checked "$TMPDIR/scenario1.$service.status.running.out" \
        nix run --no-write-lock-file "path:$workspace#svc::$service::status"
      proof_require_contains "$TMPDIR/scenario1.$service.status.running.out" "INFO:"
      proof_require_contains "$TMPDIR/scenario1.$service.status.running.out" "state=running"

      run_public_checked "$TMPDIR/scenario1.$service.ready.op.out" \
        nix run --no-write-lock-file "path:$workspace#svc::$service::ready"
      proof_require_contains "$TMPDIR/scenario1.$service.ready.op.out" "OK:"

      run_public_checked "$TMPDIR/scenario1.$service.health.op.out" \
        nix run --no-write-lock-file "path:$workspace#svc::$service::health"
      proof_require_contains "$TMPDIR/scenario1.$service.health.op.out" "OK:"
    done

    run_public_checked "$TMPDIR/scenario1.postgres.setup-db.out" \
      nix run --no-write-lock-file "path:$workspace#svc::postgres::setup-db"
    proof_require_contains "$TMPDIR/scenario1.postgres.setup-db.out" "OK:"

    run_public_checked "$TMPDIR/scenario1.minio.bucket-ensure.out" \
      nix run --no-write-lock-file "path:$workspace#svc::minio::bucket-ensure" -- proof-bucket
    proof_require_contains "$TMPDIR/scenario1.minio.bucket-ensure.out" "OK:"

    run_public_checked "$TMPDIR/scenario1.nginx.site-add.out" \
      nix run --no-write-lock-file "path:$workspace#svc::nginx::site-add" -- proof.local 127.0.0.1 3000
    proof_require_contains "$TMPDIR/scenario1.nginx.site-add.out" "OK:"

    run_public_checked "$TMPDIR/scenario1.nginx.site-list.out" \
      nix run --no-write-lock-file "path:$workspace#svc::nginx::site-list"
    proof_require_contains "$TMPDIR/scenario1.nginx.site-list.out" "proof.local"

    run_public_checked "$TMPDIR/scenario1.ready.out" \
      nix run --no-write-lock-file "path:$workspace#ready"
    run_public_checked "$TMPDIR/scenario1.health.out" \
      nix run --no-write-lock-file "path:$workspace#health"
    proof_require_contains "$TMPDIR/scenario1.ready.out" "OK:"
    proof_require_contains "$TMPDIR/scenario1.health.out" "OK:"
    if ${pkgs.gnugrep}/bin/grep -Fq "SKIP:" "$TMPDIR/scenario1.ready.out"; then
      proof_fail "scenario1 ready must not skip with enabled proof services"
    fi
    if ${pkgs.gnugrep}/bin/grep -Fq "SKIP:" "$TMPDIR/scenario1.health.out"; then
      proof_fail "scenario1 health must not skip with enabled proof services"
    fi
    for service in helios minio nginx postgres reth; do
      proof_require_contains "$TMPDIR/scenario1.ready.out" "$service"
      proof_require_contains "$TMPDIR/scenario1.health.out" "$service"
    done

    run_public_checked "$TMPDIR/scenario1.run-task.out" \
      env PROOF_HOOK_LOG_FILE="$hook_log_file" \
      nix run --no-write-lock-file "path:$workspace#run-task" -- task.test.hooks.order --run-id-file "$task_run_id_file"
    proof_require_contains "$TMPDIR/scenario1.run-task.out" "OK: hook order main complete"
    proof_require_file "$hook_log_file"
    cat > "$TMPDIR/scenario1.hooks.expected" <<'EOF'
pre-1
pre-2
main
post
EOF
    if ! ${pkgs.diffutils}/bin/diff -u "$TMPDIR/scenario1.hooks.expected" "$hook_log_file"; then
      proof_fail "unexpected hook execution order"
    fi

    run_public_checked "$TMPDIR/scenario1.run-workflow.out" \
      env PROOF_WORKFLOW_PHASE_LOG_FILE="$workflow_phase_log_file" \
      nix run --no-write-lock-file "path:$workspace#run-workflow" -- workflow.test.service.phase.probe --run-id-file "$seq_run_id_file" --summary
    proof_require_contains "$TMPDIR/scenario1.run-workflow.out" "INFO: summary_json="
    proof_require_file "$workflow_phase_log_file"
    unit_line="$(grep -n '^unit$' "$workflow_phase_log_file" | cut -d: -f1)"
    proof_require_non_empty "$unit_line" "scenario1 workflow unit marker"
    for service in helios minio nginx postgres reth; do
      ready_line="$(grep -n "^ready:$service$" "$workflow_phase_log_file" | cut -d: -f1)"
      health_line="$(grep -n "^health:$service$" "$workflow_phase_log_file" | cut -d: -f1)"
      proof_require_non_empty "$ready_line" "ready phase marker for $service"
      proof_require_non_empty "$health_line" "health phase marker for $service"
      if [ "$ready_line" -ge "$unit_line" ]; then
        proof_fail "ready phase must happen before workflow unit for $service"
      fi
      if [ "$health_line" -le "$unit_line" ]; then
        proof_fail "health phase must happen after workflow unit for $service"
      fi
    done

    run_public_checked "$TMPDIR/scenario1.run-workflow-parallel.out" \
      env NIXFIED_WORKFLOW_PARALLEL=1 NIXFIED_PARALLEL_SMOKE=1 CI_MAX_WORKERS=2 \
      nix run --no-write-lock-file "path:$workspace#run-workflow-parallel" -- workflow.test.parallel.smoke --run-id-file "$par_run_id_file"
    proof_require_contains "$TMPDIR/scenario1.run-workflow-parallel.out" "INFO: summary_json="

    proof_require_file "$task_run_id_file"
    proof_require_file "$seq_run_id_file"
    proof_require_file "$par_run_id_file"
    task_run_id="$(tr -d '\n' < "$task_run_id_file")"
    seq_run_id="$(tr -d '\n' < "$seq_run_id_file")"
    par_run_id="$(tr -d '\n' < "$par_run_id_file")"
    proof_require_non_empty "$task_run_id" "scenario1 task run id"
    proof_require_non_empty "$seq_run_id" "scenario1 workflow run id"
    proof_require_non_empty "$par_run_id" "scenario1 parallel workflow run id"

    run_public_checked "$TMPDIR/scenario1.runs.list.out" \
      nix run --no-write-lock-file "path:$workspace#runs"
    proof_require_contains "$TMPDIR/scenario1.runs.list.out" "$task_run_id"
    proof_require_contains "$TMPDIR/scenario1.runs.list.out" "$seq_run_id"
    proof_require_contains "$TMPDIR/scenario1.runs.list.out" "$par_run_id"

    read_run_record "$task_run_id" "$TMPDIR/scenario1.task.run-record.json"
    read_run_record "$seq_run_id" "$TMPDIR/scenario1.seq.run-record.json"
    read_run_record "$par_run_id" "$TMPDIR/scenario1.par.run-record.json"
    ${pkgs.jq}/bin/jq -e '.payload.command == "run-task" and .payload.state == "passed"' "$TMPDIR/scenario1.task.run-record.json" >/dev/null
    ${pkgs.jq}/bin/jq -e '.payload.command == "run-workflow" and .payload.workflow_id == "workflow.test.service.phase.probe" and .payload.state == "passed"' "$TMPDIR/scenario1.seq.run-record.json" >/dev/null
    ${pkgs.jq}/bin/jq -e '.payload.command == "run-workflow" and .payload.execution_mode == "workflow-parallel" and .payload.state == "passed"' "$TMPDIR/scenario1.par.run-record.json" >/dev/null

    for service in helios minio nginx postgres reth; do
      run_public_checked "$TMPDIR/scenario1.$service.stop.out" \
        nix run --no-write-lock-file "path:$workspace#svc::$service::stop"
      proof_require_contains "$TMPDIR/scenario1.$service.stop.out" "OK:"

      run_public_checked "$TMPDIR/scenario1.$service.status.stopped.out" \
        nix run --no-write-lock-file "path:$workspace#svc::$service::status"
      proof_require_contains "$TMPDIR/scenario1.$service.status.stopped.out" "state=stopped"
    done

    echo "OK: proof workspace scenario 1 public surface happy path passed" > "$out"
  ''
