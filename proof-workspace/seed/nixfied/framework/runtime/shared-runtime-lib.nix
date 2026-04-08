# Shared shell functions used by both executor.nix and orchestrator.nix.
#
# Returns a shell string block defining:
#   write_name_value_tsv_file()
#   kernel_run_id_envelope()
#   run_id_pass_through_env_file()  (including collect_task_env_names / collect_workflow_env_names)
#   compute_run_id()
#   activate_run()
#   deactivate_run()
#
# Callers must ensure the following variables and functions are available
# at the point this block is interpolated:
#   - REGISTRY_ROOT, RUN_ID_ACTIVE_ROOT, RUN_ID_COUNTER_ROOT
#   - RUN_SUFFIX_REASON  (set to "" by caller before compute_run_id)
#   - sha256_text()
#   - registry_lock_acquire / registry_lock_release
#   - NIXFIED_MODEL_FILE
#   - ${kernelPackage}/bin/nixfied-kernel  (interpolated by Nix)
#   - ${pkgs.coreutils}/bin/sort           (interpolated by Nix)
{
  pkgs,
  model,
  runtimeHash ? model.identity.evalHash,
  runCounterLockPurpose ? "run-counter",
}:
let
  kernelPackage = import ./kernel { inherit pkgs; };
in
''
  write_name_value_tsv_file() {
    local target_file="$1"
    local value_name=""

    : > "$target_file" || return 1
    while IFS= read -r value_name; do
      [ -n "$value_name" ] || continue
      if [ -z "''${!value_name+x}" ]; then
        continue
      fi
      printf '%s\t%s\n' "$value_name" "''${!value_name}" >> "$target_file" || return 1
    done
  }

  kernel_run_id_envelope() {
    local run_kind="$1"
    local workflow_id="$2"
    local task_id="$3"
    local slot_value="$4"
    local env_value="$5"
    local pass_through_env_file="$6"
    shift 6

    ${kernelPackage}/bin/nixfied-kernel run-id envelope \
      ${pkgs.lib.escapeShellArg "${model.identity.evalHash}"} \
      ${pkgs.lib.escapeShellArg "${runtimeHash}"} \
      "$run_kind" \
      "$workflow_id" \
      "$task_id" \
      "$slot_value" \
      "$env_value" \
      "$pass_through_env_file" \
      -- "$@"
  }

  run_id_pass_through_env_file() {
    local run_kind="$1"
    local workflow_id="$2"
    local task_id="$3"
    local env_file=""
    local env_names_file=""

    env_names_file="$(mktemp "''${TMPDIR:-/tmp}/nixfied-run-id-env-names.XXXXXX")" || return 1

    case "$run_kind" in
      task)
        ${kernelPackage}/bin/nixfied-kernel task env-names "$NIXFIED_MODEL_FILE" "$task_id"
        ;;
      workflow)
        ${kernelPackage}/bin/nixfied-kernel workflow env-names "$NIXFIED_MODEL_FILE" "$workflow_id"
        ;;
      *)
        :
        ;;
    esac | ${pkgs.coreutils}/bin/sort -u > "$env_names_file"

    env_file="$(mktemp "''${TMPDIR:-/tmp}/nixfied-run-id-env.XXXXXX")" || return 1
    if ! write_name_value_tsv_file "$env_file" < "$env_names_file"; then
      rm -f "$env_file" "$env_names_file"
      return 1
    fi
    rm -f "$env_names_file"
    printf '%s' "$env_file"
  }

  compute_run_id() {
    local run_kind="$1"
    local workflow_id="$2"
    local task_id="$3"
    shift 3

    local slot_var=${pkgs.lib.escapeShellArg model.runtime.slot.var}
    local env_var=${pkgs.lib.escapeShellArg model.runtime.env.var}
    local slot_default=${toString model.runtime.slot.default}
    local env_default=${pkgs.lib.escapeShellArg model.runtime.env.default}

    local slot_value
    local env_value
    local pass_through_env_file
    local run_input
    local run_base
    local run_id
    local lock_file
    local counter_file
    local lock_fd
    local counter="0"

    slot_value="''${!slot_var:-$slot_default}"
    env_value="''${!env_var:-$env_default}"
    pass_through_env_file="$(run_id_pass_through_env_file "$run_kind" "$workflow_id" "$task_id")" || return 1

    run_input="$(kernel_run_id_envelope "$run_kind" "$workflow_id" "$task_id" "$slot_value" "$env_value" "$pass_through_env_file" "$@")" || {
      rm -f "$pass_through_env_file"
      return 1
    }
    rm -f "$pass_through_env_file"
    run_base="$(sha256_text "$run_input")"
    run_id="run-''${run_base:0:24}"
    RUN_SUFFIX_REASON=""

    mkdir -p "$RUN_ID_ACTIVE_ROOT" "$RUN_ID_COUNTER_ROOT"
    lock_file="$RUN_ID_COUNTER_ROOT/$run_base.lock"
    counter_file="$RUN_ID_COUNTER_ROOT/$run_base"
    lock_fd="$(registry_lock_acquire "$lock_file" ${pkgs.lib.escapeShellArg "${runCounterLockPurpose}:\$run_base"} 30)" || return 1

    if [ -e "$RUN_ID_ACTIVE_ROOT/$run_id" ]; then
      if [ -f "$counter_file" ]; then
        counter="$(cat "$counter_file")"
      fi
      counter="$(( counter + 1 ))"
      printf '%s' "$counter" > "$counter_file"

      run_id="$run_id-$(printf 'c%03d' "$counter")"
      RUN_SUFFIX_REASON="active-collision"
    fi

    : > "$RUN_ID_ACTIVE_ROOT/$run_id"
    registry_lock_release "$lock_fd" "$lock_file"

    printf '%s' "$run_id"
  }

  activate_run() {
    local run_id="$1"
    mkdir -p "$RUN_ID_ACTIVE_ROOT"
    : > "$RUN_ID_ACTIVE_ROOT/$run_id"
  }

  deactivate_run() {
    local run_id="$1"
    rm -f "$RUN_ID_ACTIVE_ROOT/$run_id"
  }
''
