{ pkgs, ... }:
let
  materialize = import ../lib/materialize.nix {
    inherit pkgs;
    repoRoot = ../..;
  };
in
pkgs.runCommand "proof-workspace-scenario-5-wrapper-roundtrip"
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

    run_wrapper_checked() {
      local workspace_root="$1"
      local registry_root="$2"
      local artifacts_root="$3"
      local out_file="$4"
      shift 4
      local workspace_name=""
      local wrapper_home=""
      workspace_name="$(basename "$workspace_root")"
      wrapper_home="$TMPDIR/$workspace_name.proof-home"
      mkdir -p "$wrapper_home/.cache" "$registry_root" "$artifacts_root"
      set +e
      HOME="$wrapper_home" XDG_CACHE_HOME="$wrapper_home/.cache" \
        REGISTRY_ROOT="$registry_root" CI_ARTIFACTS_ROOT="$artifacts_root" \
        "$@" > "$out_file" 2>&1
      local rc="$?"
      set -e
      if [ "$rc" -ne 0 ]; then
        cat "$out_file" 2>/dev/null || true
        proof_fail "wrapper command failed rc=$rc"
      fi
    }

    read_wrapper_run_record() {
      local workspace_root="$1"
      local registry_root="$2"
      local artifacts_root="$3"
      local run_id="$4"
      local out_file="$5"
      local raw_out="$out_file.raw"
      local run_json=""
      run_wrapper_checked "$workspace_root" "$registry_root" "$artifacts_root" "$raw_out" \
        nix run --offline --no-write-lock-file "path:$workspace_root#runs" -- "$run_id"
      run_json="$(${pkgs.gnused}/bin/sed -n '/^{/p' "$raw_out" | ${pkgs.coreutils}/bin/tail -n 1)"
      proof_require_non_empty "$run_json" "wrapper run record json for $run_id"
      printf '%s\n' "$run_json" > "$out_file"
    }

    run_wrapper_proof_flow() {
      local workspace_root="$1"
      local label="$2"
      local flow_root="$TMPDIR/scenario5.$label"
      local registry_root="$flow_root/registry"
      local artifacts_root="$flow_root/artifacts"
      local workflow_phase_log_file="$flow_root/workflow-phase.log"
      local run_id_file="$flow_root/workflow.run-id"
      local workflow_run_id=""
      local unit_line=""

      mkdir -p "$flow_root" "$registry_root" "$artifacts_root"

      run_wrapper_checked "$workspace_root" "$registry_root" "$artifacts_root" "$flow_root.validate.out" \
        nix run --offline --no-write-lock-file "path:$workspace_root#validate-env"
      proof_require_contains "$flow_root.validate.out" "OK: environment is valid"

      run_wrapper_checked "$workspace_root" "$registry_root" "$artifacts_root" "$flow_root.helios.full-start.out" \
        nix run --offline --no-write-lock-file "path:$workspace_root#svc::helios::full-start"
      proof_require_contains "$flow_root.helios.full-start.out" "OK:"

      run_wrapper_checked "$workspace_root" "$registry_root" "$artifacts_root" "$flow_root.helios.status.running.out" \
        nix run --offline --no-write-lock-file "path:$workspace_root#svc::helios::status"
      proof_require_contains "$flow_root.helios.status.running.out" "INFO:"
      proof_require_contains "$flow_root.helios.status.running.out" "state=running"

      run_wrapper_checked "$workspace_root" "$registry_root" "$artifacts_root" "$flow_root.ready.out" \
        nix run --offline --no-write-lock-file "path:$workspace_root#ready" -- --service helios
      proof_require_contains "$flow_root.ready.out" "OK: readiness checks passed services=1"
      proof_require_contains "$flow_root.ready.out" "helios"
      if ${pkgs.gnugrep}/bin/grep -Fq "SKIP: helios readiness check not selected" "$flow_root.ready.out"; then
        proof_fail "wrapper proof flow ready must select helios"
      fi

      run_wrapper_checked "$workspace_root" "$registry_root" "$artifacts_root" "$flow_root.health.out" \
        nix run --offline --no-write-lock-file "path:$workspace_root#health" -- --service helios
      proof_require_contains "$flow_root.health.out" "OK: health checks passed services=1"
      proof_require_contains "$flow_root.health.out" "helios"
      if ${pkgs.gnugrep}/bin/grep -Fq "SKIP: helios health check not selected" "$flow_root.health.out"; then
        proof_fail "wrapper proof flow health must select helios"
      fi

      run_wrapper_checked "$workspace_root" "$registry_root" "$artifacts_root" "$flow_root.workflow.out" \
        env PROOF_WORKFLOW_PHASE_LOG_FILE="$workflow_phase_log_file" \
        nix run --offline --no-write-lock-file "path:$workspace_root#run-workflow" -- workflow.test.service.phase.probe --run-id-file "$run_id_file" --summary
      proof_require_contains "$flow_root.workflow.out" "INFO: summary_json="
      proof_require_file "$run_id_file"
      proof_require_file "$workflow_phase_log_file"

      workflow_run_id="$(tr -d '\n' < "$run_id_file")"
      proof_require_non_empty "$workflow_run_id" "scenario5 workflow run id for $label"
      read_wrapper_run_record \
        "$workspace_root" \
        "$registry_root" \
        "$artifacts_root" \
        "$workflow_run_id" \
        "$flow_root.workflow.run-record.json"
      ${pkgs.jq}/bin/jq -e '
        .payload.command == "run-workflow"
        and .payload.workflow_id == "workflow.test.service.phase.probe"
        and .payload.state == "passed"
      ' "$flow_root.workflow.run-record.json" >/dev/null

      unit_line="$(${pkgs.gnugrep}/bin/grep -n '^unit$' "$workflow_phase_log_file" | cut -d: -f1)"
      proof_require_non_empty "$unit_line" "scenario5 workflow unit marker for $label"
      for service in helios minio nginx postgres reth; do
        ready_line="$(${pkgs.gnugrep}/bin/grep -n "^ready:$service$" "$workflow_phase_log_file" | cut -d: -f1)"
        health_line="$(${pkgs.gnugrep}/bin/grep -n "^health:$service$" "$workflow_phase_log_file" | cut -d: -f1)"
        proof_require_non_empty "$ready_line" "scenario5 ready phase marker for $service ($label)"
        proof_require_non_empty "$health_line" "scenario5 health phase marker for $service ($label)"
        if [ "$ready_line" -ge "$unit_line" ]; then
          proof_fail "wrapper proof flow ready phase must happen before workflow unit for $service ($label)"
        fi
        if [ "$health_line" -le "$unit_line" ]; then
          proof_fail "wrapper proof flow health phase must happen after workflow unit for $service ($label)"
        fi
      done

      run_wrapper_checked "$workspace_root" "$registry_root" "$artifacts_root" "$flow_root.runs.list.out" \
        nix run --offline --no-write-lock-file "path:$workspace_root#runs"
      proof_require_contains "$flow_root.runs.list.out" "$workflow_run_id"

      for service in helios minio nginx postgres reth; do
        run_wrapper_checked "$workspace_root" "$registry_root" "$artifacts_root" "$flow_root.$service.stop.out" \
          nix run --offline --no-write-lock-file "path:$workspace_root#svc::$service::stop"
        proof_require_contains "$flow_root.$service.stop.out" "OK:"

        run_wrapper_checked "$workspace_root" "$registry_root" "$artifacts_root" "$flow_root.$service.status.stopped.out" \
          nix run --offline --no-write-lock-file "path:$workspace_root#svc::$service::status"
        proof_require_contains "$flow_root.$service.status.stopped.out" "state=stopped"
      done
    }

    thin_workspace="$TMPDIR/proof-install-thin"
    vendor_workspace="$TMPDIR/proof-install-vendor"

    proof_workspace_bootstrap_install "$thin_workspace" thin
    proof_workspace_bootstrap_install "$vendor_workspace" vendor

    proof_require_dir "$thin_workspace/.git"
    proof_require_file "$thin_workspace/flake.nix"
    proof_require_dir "$vendor_workspace/.git"
    proof_require_file "$vendor_workspace/flake.nix"

    run_wrapper_proof_flow "$thin_workspace" "thin"
    run_wrapper_proof_flow "$vendor_workspace" "vendor.pre-upgrade"

    project_file="$vendor_workspace/nixfied/project/module.nix"
    local_file="$vendor_workspace/nixfied/local/default.nix"
    vendored_metadata_file="$vendor_workspace/nixfied/VENDORED.txt"

    proof_require_file "$project_file"
    proof_require_file "$local_file"
    proof_require_file "$vendored_metadata_file"
    proof_require_contains "$vendored_metadata_file" "Framework source revision (install/upgrade):"

    printf '\n# PROOF_USER_PROJECT_MARKER\n' >> "$project_file"
    printf '\n# PROOF_USER_LOCAL_MARKER\n' >> "$local_file"
    printf '\n# PROOF_FRAMEWORK_VENDORED_MARKER\n' >> "$vendored_metadata_file"

    run_wrapper_checked \
      "$vendor_workspace" \
      "$TMPDIR/scenario5.vendor-upgrade.registry" \
      "$TMPDIR/scenario5.vendor-upgrade.artifacts" \
      "$TMPDIR/scenario5.vendor.upgrade.out" \
      nix run --offline --no-write-lock-file "path:$vendor_workspace#framework::upgrade" -- --target "$vendor_workspace"

    proof_require_contains "$project_file" "PROOF_USER_PROJECT_MARKER"
    proof_require_contains "$local_file" "PROOF_USER_LOCAL_MARKER"
    if ${pkgs.gnugrep}/bin/grep -Fq "PROOF_FRAMEWORK_VENDORED_MARKER" "$vendored_metadata_file"; then
      proof_fail "framework-owned vendored metadata marker should be removed by upgrade"
    fi

    proof_require_file "$vendored_metadata_file"
    proof_require_contains "$vendored_metadata_file" "Framework source revision (install/upgrade):"
    if ! ${pkgs.gnugrep}/bin/grep -Eq '^- [0-9a-f]{7,}(-dirty)?$' "$vendored_metadata_file"; then
      proof_fail "vendored metadata must contain a framework source revision entry"
    fi

    run_wrapper_proof_flow "$vendor_workspace" "vendor.post-upgrade"

    echo "OK: proof workspace scenario 5 wrapper roundtrip passed" > "$out"
  ''
