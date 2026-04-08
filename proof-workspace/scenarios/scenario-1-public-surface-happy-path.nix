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
        proof_fail "public command failed: $*"
      fi
    }

    read_run_record() {
      local run_id="$1"
      local out_file="$2"
      local raw_out="$out_file.raw"
      local run_json=""
      run_public_checked "$raw_out" nix run "path:$workspace#runs" -- "$run_id"
      run_json="$(${pkgs.gnused}/bin/sed -n '/^{/p' "$raw_out" | ${pkgs.coreutils}/bin/tail -n 1)"
      proof_require_non_empty "$run_json" "run record json for $run_id"
      printf '%s\n' "$run_json" > "$out_file"
    }

    run_public_checked "$TMPDIR/scenario1.help.out" \
      nix run "path:$workspace#help"
    run_public_checked "$TMPDIR/scenario1.features.out" \
      nix run "path:$workspace#features"
    run_public_checked "$TMPDIR/scenario1.introspect.out" \
      nix run "path:$workspace#introspect" -- app:validate-env --json
    run_public_checked "$TMPDIR/scenario1.validate-env.out" \
      nix run "path:$workspace#validate-env"
    run_public_checked "$TMPDIR/scenario1.ports.out" \
      nix run "path:$workspace#ports"
    run_public_checked "$TMPDIR/scenario1.check-ports.out" \
      nix run "path:$workspace#check-ports"
    run_public_checked "$TMPDIR/scenario1.ready.out" \
      nix run "path:$workspace#ready"
    run_public_checked "$TMPDIR/scenario1.health.out" \
      nix run "path:$workspace#health"

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
    proof_require_contains "$TMPDIR/scenario1.ready.out" "SKIP:"
    proof_require_contains "$TMPDIR/scenario1.health.out" "SKIP:"

    task_run_id_file="$TMPDIR/scenario1.task.run-id"
    seq_run_id_file="$TMPDIR/scenario1.seq.run-id"
    par_run_id_file="$TMPDIR/scenario1.par.run-id"

    run_public_checked "$TMPDIR/scenario1.run-task.out" \
      nix run "path:$workspace#run-task" -- task.test.isolation.unit --run-id-file "$task_run_id_file"
    proof_require_contains "$TMPDIR/scenario1.run-task.out" "OK: isolation probe complete"

    run_public_checked "$TMPDIR/scenario1.run-workflow.out" \
      nix run "path:$workspace#run-workflow" -- workflow.test.isolation.probe --run-id-file "$seq_run_id_file" --summary
    proof_require_contains "$TMPDIR/scenario1.run-workflow.out" "INFO: summary_json="

    run_public_checked "$TMPDIR/scenario1.run-workflow-parallel.out" \
      env NIXFIED_WORKFLOW_PARALLEL=1 NIXFIED_PARALLEL_SMOKE=1 CI_MAX_WORKERS=2 \
      nix run "path:$workspace#run-workflow-parallel" -- workflow.test.parallel.smoke --run-id-file "$par_run_id_file"
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
      nix run "path:$workspace#runs"
    proof_require_contains "$TMPDIR/scenario1.runs.list.out" "$task_run_id"
    proof_require_contains "$TMPDIR/scenario1.runs.list.out" "$seq_run_id"
    proof_require_contains "$TMPDIR/scenario1.runs.list.out" "$par_run_id"

    read_run_record "$task_run_id" "$TMPDIR/scenario1.task.run-record.json"
    read_run_record "$seq_run_id" "$TMPDIR/scenario1.seq.run-record.json"
    read_run_record "$par_run_id" "$TMPDIR/scenario1.par.run-record.json"
    ${pkgs.jq}/bin/jq -e '.payload.command == "run-task" and .payload.state == "passed"' "$TMPDIR/scenario1.task.run-record.json" >/dev/null
    ${pkgs.jq}/bin/jq -e '.payload.command == "run-workflow" and .payload.state == "passed"' "$TMPDIR/scenario1.seq.run-record.json" >/dev/null
    ${pkgs.jq}/bin/jq -e '.payload.command == "run-workflow" and .payload.execution_mode == "workflow-parallel" and .payload.state == "passed"' "$TMPDIR/scenario1.par.run-record.json" >/dev/null

    echo "OK: proof workspace scenario 1 public surface happy path passed" > "$out"
  ''
