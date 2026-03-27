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
#   - task_runtime_pass_through_env_names, task_hook_runtime_pass_through_env_names,
#     task_hook_ids, task_needs, task_soft_needs, task_runner_type,
#     task_runner_workflow_id, workflow_phase_tasks, workflow_plan_task_ids
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
    local env_name=""
    local dep_task_id=""
    local phase=""
    local hook_id=""
    local runner_type=""
    local nested_workflow_id=""
    local unit_json=""
    local unit_task_id=""
    local -A seen_tasks
    local -A seen_workflows

    collect_task_env_names() {
      local current_task_id="$1"

      if [ -z "$current_task_id" ] || [ -n "''${seen_tasks[$current_task_id]:-}" ]; then
        return 0
      fi
      seen_tasks[$current_task_id]=1

      task_runtime_pass_through_env_names "$current_task_id"

      for phase in pre post; do
        while IFS= read -r hook_id; do
          [ -n "$hook_id" ] || continue
          task_hook_runtime_pass_through_env_names "$current_task_id" "$phase" "$hook_id"
        done < <(task_hook_ids "$current_task_id" "$phase" 2>/dev/null || true)
      done

      while IFS= read -r dep_task_id; do
        [ -n "$dep_task_id" ] || continue
        collect_task_env_names "$dep_task_id"
      done < <(task_needs "$current_task_id" 2>/dev/null || true)

      while IFS= read -r dep_task_id; do
        [ -n "$dep_task_id" ] || continue
        collect_task_env_names "$dep_task_id"
      done < <(task_soft_needs "$current_task_id" 2>/dev/null || true)

      runner_type="$(task_runner_type "$current_task_id")"
      if [ "$runner_type" = "workflowRef" ]; then
        nested_workflow_id="$(task_runner_workflow_id "$current_task_id")"
        if [ -n "$nested_workflow_id" ]; then
          collect_workflow_env_names "$nested_workflow_id"
        fi
      fi
    }

    collect_workflow_env_names() {
      local current_workflow_id="$1"

      if [ -z "$current_workflow_id" ] || [ -n "''${seen_workflows[$current_workflow_id]:-}" ]; then
        return 0
      fi
      seen_workflows[$current_workflow_id]=1

      while IFS= read -r dep_task_id; do
        [ -n "$dep_task_id" ] || continue
        collect_task_env_names "$dep_task_id"
      done < <(workflow_phase_tasks "$current_workflow_id" preRun 2>/dev/null || true)

      while IFS= read -r unit_task_id; do
        [ -n "$unit_task_id" ] || continue
        collect_task_env_names "$unit_task_id"
      done < <(workflow_plan_task_ids "$current_workflow_id" 2>/dev/null || true)

      while IFS= read -r dep_task_id; do
        [ -n "$dep_task_id" ] || continue
        collect_task_env_names "$dep_task_id"
      done < <(workflow_phase_tasks "$current_workflow_id" postRun 2>/dev/null || true)
    }

    env_names_file="$(mktemp "''${TMPDIR:-/tmp}/nixfied-run-id-env-names.XXXXXX")" || return 1

    case "$run_kind" in
      task)
        collect_task_env_names "$task_id"
        ;;
      workflow)
        collect_workflow_env_names "$workflow_id"
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
