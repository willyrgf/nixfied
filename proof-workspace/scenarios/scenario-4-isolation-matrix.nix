{ pkgs, ... }:
let
  materialize = import ../lib/materialize.nix {
    inherit pkgs;
    repoRoot = ../..;
  };
in
pkgs.runCommand "proof-workspace-scenario-4-isolation-matrix"
  {
    nativeBuildInputs = [
      pkgs.coreutils
      pkgs.findutils
      pkgs.git
      pkgs.gnugrep
      pkgs.gnused
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

    conf_file="$workspace/nixfied/project/conf.nix"
    scenario4_logs_base="$TMPDIR/isolation-logs"
    rm -rf "$scenario4_logs_base"
    mkdir -p "$scenario4_logs_base"

    ${pkgs.gnused}/bin/sed -i.bak \
      -e "s|^\([[:space:]]*\)logsDir = \".*\";|\1logsDir = \"$scenario4_logs_base\";|" \
      -e 's/keepLogsOnSuccess = false;/keepLogsOnSuccess = true;/' \
      "$conf_file"
    rm -f "$conf_file.bak"

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

    logs_roots_file="$TMPDIR/scenario4.logs-roots.list"
    : > "$logs_roots_file"
    cell_count=0

    for slot_value in 5 7; do
      for env_value in dev test prod; do
        cell_count=$((cell_count + 1))
        cell_out="$TMPDIR/scenario4.slot-$slot_value.env-$env_value.out"
        logs_root=""
        cell_dir=""
        summary_json=""
        run_id=""

        run_public_checked "$cell_out" \
          nix run "path:$workspace#test-isolation" -- --slot "$slot_value" --env "$env_value" --max-parallel 1
        proof_require_contains "$cell_out" "INFO: test-isolation matrix slots=1 envs=1 max_parallel=1"
        proof_require_contains "$cell_out" "INFO: isolation cell start slot=$slot_value env=$env_value"
        proof_require_contains "$cell_out" "OK: isolation cell passed slot=$slot_value env=$env_value"
        proof_require_contains "$cell_out" "OK: test-isolation completed total=1"

        logs_root="$(${pkgs.gnused}/bin/sed -n 's/^INFO: test-isolation logs_root=//p' "$cell_out" | ${pkgs.coreutils}/bin/tail -n 1)"
        proof_require_non_empty "$logs_root" "scenario4 logs_root slot=$slot_value env=$env_value"
        proof_require_dir "$logs_root"
        printf '%s\n' "$logs_root" >> "$logs_roots_file"

        cell_dir="$logs_root/slot-''${slot_value}__env-''${env_value}"
        proof_require_dir "$cell_dir"
        proof_require_file "$cell_dir/run.log"
        proof_require_file "$cell_dir/validate.log"
        proof_require_file "$cell_dir/run.run-id"
        proof_require_file "$cell_dir/summary.json"
        proof_require_file "$cell_dir/artifacts/summary.json"
        proof_require_file "$cell_dir/registry/events.ndjson"

        proof_require_contains "$cell_dir/run.log" "OK: isolation probe complete slot=$slot_value env=$env_value"
        proof_require_contains "$cell_dir/validate.log" "OK: environment is valid (PROJECT_ENV=$env_value NIX_ENV=$slot_value)"

        run_id="$(tr -d '\n' < "$cell_dir/run.run-id")"
        proof_require_non_empty "$run_id" "scenario4 run id slot=$slot_value env=$env_value"
        proof_require_contains "$cell_dir/registry/events.ndjson" "$run_id"

        summary_json="$(${pkgs.gnused}/bin/sed -n 's/^INFO: summary_json=//p' "$cell_dir/run.log" | ${pkgs.coreutils}/bin/tail -n 1)"
        proof_require_non_empty "$summary_json" "scenario4 summary_json slot=$slot_value env=$env_value"
        proof_require_file "$summary_json"
        case "$summary_json" in
          "$cell_dir/artifacts"/*) ;;
          *)
            proof_fail "summary path escaped cell artifacts root for slot=$slot_value env=$env_value: $summary_json"
            ;;
        esac
      done
    done

    unique_logs_roots="$(${pkgs.coreutils}/bin/sort "$logs_roots_file" | ${pkgs.coreutils}/bin/uniq | ${pkgs.coreutils}/bin/wc -l | tr -d ' ')"
    if [ "$unique_logs_roots" -ne "$cell_count" ]; then
      proof_fail "expected unique logs_root per cell (cells=$cell_count unique_logs_roots=$unique_logs_roots)"
    fi

    unique_cell_names="$(${pkgs.findutils}/bin/find "$scenario4_logs_base" -mindepth 2 -maxdepth 2 -type d -name 'slot-*__env-*' | ${pkgs.gnused}/bin/sed 's|.*/||' | ${pkgs.coreutils}/bin/sort | ${pkgs.coreutils}/bin/uniq | ${pkgs.coreutils}/bin/wc -l | tr -d ' ')"
    if [ "$unique_cell_names" -ne "$cell_count" ]; then
      proof_fail "expected unique slot/env cells (cells=$cell_count unique_cells=$unique_cell_names)"
    fi

    echo "OK: proof workspace scenario 4 isolation matrix passed" > "$out"
  ''
