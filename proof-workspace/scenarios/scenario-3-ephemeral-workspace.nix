{ pkgs, ... }:
let
  materialize = import ../lib/materialize.nix {
    inherit pkgs;
    repoRoot = ../..;
  };
in
pkgs.runCommand "proof-workspace-scenario-3-ephemeral-workspace"
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

    scenario_module="$workspace/nixfied/project/proof-scenario3-module.nix"
    module_file="$workspace/nixfied/project/module.nix"

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

    if ! ${pkgs.gnugrep}/bin/grep -Fq "./proof-scenario3-module.nix" "$module_file"; then
      ${pkgs.gnused}/bin/sed -i.bak '/^[[:space:]]*projectWorkflowsModule$/a\
    ./proof-scenario3-module.nix' "$module_file"
      rm -f "$module_file.bak"
    fi

    printf 'tracked\n' > "$workspace/tracked.txt"
    ${pkgs.git}/bin/git -C "$workspace" add tracked.txt "$module_file"
    printf 'untracked\n' > "$workspace/keep-untracked.txt"
    printf 'EPHEMERAL_HOST_ENV_SECRET=from-host-env\n' > "$workspace/.ephemeral-secret.env"

    write_probe_module() {
      local marker="$1"
      local copy_mode="$2"
      local include_untracked="$3"
      local env_mode="$4"
      local expect_untracked="$5"
      local expect_env="$6"
      cat > "$scenario_module" <<'EOF'
{ lib, ... }:
{
  config = {
    nixfied.runtime.ephemeral = {
      copyMode = lib.mkForce "__COPY_MODE__";
      includeUntracked = lib.mkForce __INCLUDE_UNTRACKED__;
      envFileMode = lib.mkForce "__ENV_MODE__";
      envFilePath = lib.mkForce ".ephemeral-secret.env";
    };

    nixfied.tasks."test.proof.scenario3.probe" = {
      id = "task.test.proof.scenario3.probe";
      summary = "proof scenario 3 ephemeral probe";
      description = "proof scenario 3 ephemeral probe";
      runtime = {
        passThroughEnv = [ "EPHEMERAL_HOST_ENV_SECRET" ];
        allowSensitivePassThrough = true;
      };
      runner.command = '''
        set -euo pipefail
        test -f tracked.txt
        if [ "__EXPECT_UNTRACKED__" = "true" ]; then
          test -f keep-untracked.txt
        else
          test ! -e keep-untracked.txt
        fi
        if [ "__EXPECT_ENV__" = "true" ]; then
          test "$(printenv EPHEMERAL_HOST_ENV_SECRET)" = "from-host-env"
        else
          if printenv EPHEMERAL_HOST_ENV_SECRET >/dev/null 2>&1; then
            exit 1
          fi
        fi
        echo "OK: __MARKER__"
      ''';
    };

    nixfied.workflows."test.proof.scenario3.probe" = {
      id = "workflow.test.proof.scenario3.probe";
      summary = "proof scenario 3 ephemeral probe workflow";
      description = "proof scenario 3 ephemeral probe workflow";
      mode = "custom";
      maxWorkers = 1;
      units.probe.taskId = "task.test.proof.scenario3.probe";
      preRun.tasks = [ ];
      postRun = {
        tasks = [ ];
        alwaysRun = true;
      };
      artifacts = {
        keepOnSuccess = false;
        keepOnFailure = true;
        writeSummary = true;
      };
      execution = {
        parallel = false;
        failFast = true;
        lockPolicy = "exclusive";
        emitRegistryEvents = true;
        ephemeral.enable = true;
      };
    };
  };
}
EOF
      ${pkgs.gnused}/bin/sed -i.bak \
        -e "s|__COPY_MODE__|$copy_mode|g" \
        -e "s|__INCLUDE_UNTRACKED__|$include_untracked|g" \
        -e "s|__ENV_MODE__|$env_mode|g" \
        -e "s|__EXPECT_UNTRACKED__|$expect_untracked|g" \
        -e "s|__EXPECT_ENV__|$expect_env|g" \
        -e "s|__MARKER__|$marker|g" \
        "$scenario_module"
      rm -f "$scenario_module.bak"
    }

    run_variant() {
      local label="$1"
      local marker="$2"
      local copy_mode="$3"
      local include_untracked="$4"
      local env_mode="$5"
      local expect_untracked="$6"
      local expect_env="$7"
      local copy_log="$8"
      local env_log="$9"
      local run_out="$TMPDIR/scenario3.$label.out"
      local run_id_file="$TMPDIR/scenario3.$label.run-id"
      local run_id=""
      local summary_json=""

      write_probe_module "$marker" "$copy_mode" "$include_untracked" "$env_mode" "$expect_untracked" "$expect_env"

      run_public_checked "$run_out" \
        nix run "path:$workspace#run-workflow" -- workflow.test.proof.scenario3.probe --run-id-file "$run_id_file" --summary
      proof_require_contains "$run_out" "$copy_log"
      proof_require_contains "$run_out" "$env_log"
      proof_require_contains "$run_out" "OK: $marker"
      proof_require_file "$run_id_file"

      run_id="$(tr -d '\n' < "$run_id_file")"
      proof_require_non_empty "$run_id" "scenario3 run id ($label)"
      proof_require_file "$REGISTRY_ROOT/events.ndjson"

      read_run_record "$run_id" "$TMPDIR/scenario3.$label.run-record.json"
      ${pkgs.jq}/bin/jq -e '
        .payload.command == "run-workflow"
        and .payload.state == "passed"
        and .payload.workflow_id == "workflow.test.proof.scenario3.probe"
      ' "$TMPDIR/scenario3.$label.run-record.json" >/dev/null

      summary_json="$(${pkgs.gnused}/bin/sed -n 's/^INFO: summary_json=//p' "$run_out" | ${pkgs.coreutils}/bin/tail -n 1)"
      proof_require_non_empty "$summary_json" "summary_json for scenario3 variant $label"
      proof_require_file "$summary_json"
      case "$summary_json" in
        "$CI_ARTIFACTS_ROOT"/*) ;;
        *)
          proof_fail "summary path escaped CI_ARTIFACTS_ROOT for scenario3 variant $label: $summary_json"
          ;;
      esac
    }

    write_probe_module \
      "proof scenario 3 tracked-only copy mode" \
      "git-files" \
      "false" \
      "disabled" \
      "false" \
      "false"
    ${pkgs.git}/bin/git -C "$workspace" add "$scenario_module"
    ${pkgs.git}/bin/git -C "$workspace" \
      -c user.name=nixfied-proof \
      -c user.email=nixfied-proof@example.invalid \
      commit -m "proof scenario 3 fixture bootstrap" >/dev/null 2>&1

    run_variant \
      "tracked" \
      "proof scenario 3 tracked-only copy mode" \
      "git-files" \
      "false" \
      "disabled" \
      "false" \
      "false" \
      "INFO: Using git-files copy mode include_untracked=0" \
      "INFO: Skipping host env file import mode=disabled"

    run_variant \
      "worktree" \
      "proof scenario 3 worktree copy mode" \
      "git-files" \
      "true" \
      "disabled" \
      "true" \
      "false" \
      "INFO: Using git-files copy mode include_untracked=1" \
      "INFO: Skipping host env file import mode=disabled"

    run_variant \
      "env-original-root" \
      "proof scenario 3 env-file original-root" \
      "git-files" \
      "false" \
      "original-root" \
      "false" \
      "true" \
      "INFO: Using git-files copy mode include_untracked=0" \
      "INFO: Loading host env file mode=original-root path="

    echo "OK: proof workspace scenario 3 ephemeral workspace passed" > "$out"
  ''
