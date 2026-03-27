{ pkgs }:
let
  kernelPackage = import ./kernel { inherit pkgs; };
in
''
  NIXFIED_TASK_RUNTIME_CACHE_ID=""
  NIXFIED_TASK_HOOK_CACHE_KEY=""
  NIXFIED_WORKFLOW_RUNTIME_CACHE_ID=""

  _nixfied_kernel_eval_exports() {
    local export_text=""

    if ! export_text="$("$@")"; then
      return 1
    fi

    if [ -z "$export_text" ]; then
      return 1
    fi

    eval "$export_text"
  }

  _nixfied_task_load_cache() {
    local task_id="$1"

    if [ "$NIXFIED_TASK_RUNTIME_CACHE_ID" = "$task_id" ]; then
      return 0
    fi

    _nixfied_kernel_eval_exports \
      ${kernelPackage}/bin/nixfied-kernel task load-runtime \
      "$NIXFIED_MODEL_FILE" \
      "$task_id" || return 1
    NIXFIED_TASK_RUNTIME_CACHE_ID="$task_id"
    return 0
  }

  _nixfied_task_load_hook_cache() {
    local task_id="$1"
    local phase="$2"
    local hook_id="$3"
    local cache_key="$task_id:$phase:$hook_id"

    if [ "$NIXFIED_TASK_HOOK_CACHE_KEY" = "$cache_key" ]; then
      return 0
    fi

    _nixfied_kernel_eval_exports \
      ${kernelPackage}/bin/nixfied-kernel task load-hook \
      "$NIXFIED_MODEL_FILE" \
      "$task_id" \
      "$phase" \
      "$hook_id" || return 1
    NIXFIED_TASK_HOOK_CACHE_KEY="$cache_key"
    return 0
  }

  _nixfied_workflow_load_cache() {
    local workflow_id="$1"

    if [ "$NIXFIED_WORKFLOW_RUNTIME_CACHE_ID" = "$workflow_id" ]; then
      return 0
    fi

    _nixfied_kernel_eval_exports \
      ${kernelPackage}/bin/nixfied-kernel workflow load-runtime \
      "$NIXFIED_MODEL_FILE" \
      "$workflow_id" || return 1
    NIXFIED_WORKFLOW_RUNTIME_CACHE_ID="$workflow_id"
    return 0
  }

  task_help_requested() {
    local arg=""

    while [ "$#" -gt 0 ]; do
      arg="$1"
      shift

      case "$arg" in
        --help|-h)
          return 0
          ;;
        --)
          return 1
          ;;
      esac
    done

    return 1
  }

  task_descriptor_exists() {
    ${kernelPackage}/bin/nixfied-kernel task exists "$NIXFIED_MODEL_FILE" "$1" >/dev/null 2>&1
  }

  task_print_help() {
    ${kernelPackage}/bin/nixfied-kernel task render-help "$NIXFIED_MODEL_FILE" "$1"
  }

  task_validate_args() {
    local task_id="$1"
    shift

    ${kernelPackage}/bin/nixfied-kernel task validate-args \
      "$NIXFIED_MODEL_FILE" \
      "$task_id" \
      -- "$@"
  }

  task_runner_type() {
    _nixfied_task_load_cache "$1" || return 1
    printf '%s' "$TASK_RUNNER_TYPE"
  }

  task_runner_workflow_id() {
    _nixfied_task_load_cache "$1" || return 1
    printf '%s' "$TASK_RUNNER_WORKFLOW_ID"
  }

  task_required_services() {
    _nixfied_task_load_cache "$1" || return 1
    printf '%s\n' "$TASK_REQUIRED_SERVICES_LINES"
  }

  task_closure_selected_services() {
    _nixfied_task_load_cache "$1" || return 1
    printf '%s\n' "$TASK_CLOSURE_SELECTED_SERVICES_LINES"
  }

  task_base_closure_selected_services() {
    _nixfied_task_load_cache "$1" || return 1
    printf '%s\n' "$TASK_BASE_CLOSURE_SELECTED_SERVICES_LINES"
  }

  workflow_unit_closure_selected_services() {
    _nixfied_workflow_load_cache "$1" || return 1
    printf '%s\n' "$WORKFLOW_UNIT_CLOSURE_SELECTED_SERVICES_LINES"
  }

  workflow_plan_task_ids() {
    _nixfied_workflow_load_cache "$1" || return 1
    printf '%s\n' "$WORKFLOW_PLAN_TASK_IDS_LINES"
  }

  workflow_phase_tasks() {
    _nixfied_workflow_load_cache "$1" || return 1
    case "$2" in
      preRun)
        printf '%s\n' "$WORKFLOW_PHASE_PRE_TASKS_LINES"
        ;;
      postRun)
        printf '%s\n' "$WORKFLOW_PHASE_POST_TASKS_LINES"
        ;;
      *)
        return 1
        ;;
    esac
  }

  workflow_id_exists() {
    ${kernelPackage}/bin/nixfied-kernel workflow load-runtime "$NIXFIED_MODEL_FILE" "$1" >/dev/null 2>&1
  }

  workflow_mode_name() {
    _nixfied_workflow_load_cache "$1" || return 1
    printf '%s' "$WORKFLOW_MODE_NAME"
  }

  workflow_artifacts_root() {
    _nixfied_workflow_load_cache "$1" || return 1
    printf '%s' "$WORKFLOW_ARTIFACTS_ROOT"
  }

  workflow_ephemeral_flag() {
    _nixfied_workflow_load_cache "$1" || return 1
    printf '%s' "$WORKFLOW_EPHEMERAL_FLAG"
  }

  workflow_logging_level_default() {
    _nixfied_workflow_load_cache "$1" || return 1
    printf '%s' "$WORKFLOW_LOGGING_LEVEL_DEFAULT"
  }

  workflow_logging_output_default() {
    _nixfied_workflow_load_cache "$1" || return 1
    printf '%s' "$WORKFLOW_LOGGING_OUTPUT_DEFAULT"
  }

  workflow_fail_fast() {
    _nixfied_workflow_load_cache "$1" || return 1
    printf '%s' "$WORKFLOW_FAIL_FAST"
  }

  workflow_parallel_enabled() {
    _nixfied_workflow_load_cache "$1" || return 1
    printf '%s' "$WORKFLOW_PARALLEL_ENABLED"
  }

  workflow_max_workers() {
    _nixfied_workflow_load_cache "$1" || return 1
    printf '%s' "$WORKFLOW_MAX_WORKERS"
  }

  workflow_write_summary() {
    _nixfied_workflow_load_cache "$1" || return 1
    printf '%s' "$WORKFLOW_WRITE_SUMMARY"
  }

  workflow_post_run_always() {
    _nixfied_workflow_load_cache "$1" || return 1
    printf '%s' "$WORKFLOW_POST_RUN_ALWAYS"
  }

  workflow_simple_shorthand_exists_for_family() {
    local workflow_id="$1"
    local candidate="$2"

    ${kernelPackage}/bin/nixfied-kernel workflow resolve-mode \
      "$NIXFIED_MODEL_FILE" \
      "$workflow_id" \
      "$candidate" >/dev/null 2>&1
  }

  workflow_resolve_mode_id() {
    ${kernelPackage}/bin/nixfied-kernel workflow resolve-mode \
      "$NIXFIED_MODEL_FILE" \
      "$1" \
      "$2"
  }

  task_invocation_selected_services() {
    local task_id="$1"
    local resolved_workflow_id="''${2:-}"

    task_base_closure_selected_services "$task_id"
    if [ -n "$resolved_workflow_id" ]; then
      workflow_unit_closure_selected_services "$resolved_workflow_id"
    fi
  }

  task_runner_command() {
    _nixfied_task_load_cache "$1" || return 1
    printf '%s' "$TASK_RUNNER_COMMAND"
  }

  task_runner_package() {
    _nixfied_task_load_cache "$1" || return 1
    printf '%s' "$TASK_RUNNER_PACKAGE"
  }

  task_runtime_plan_shell() {
    _nixfied_task_load_cache "$1" || return 1
    printf '%s' "$TASK_RUNTIME_PLAN_SHELL"
  }

  task_runtime_pass_through_env_names() {
    _nixfied_task_load_cache "$1" || return 1
    printf '%s\n' "$TASK_PASS_THROUGH_ENV_NAMES_LINES"
  }

  task_produces_json() {
    _nixfied_task_load_cache "$1" || return 1
    printf '%s' "$TASK_PRODUCES_JSON"
  }

  task_max_attempts() {
    _nixfied_task_load_cache "$1" || return 1
    printf '%s' "$TASK_MAX_ATTEMPTS"
  }

  task_retry_backoff_values() {
    _nixfied_task_load_cache "$1" || return 1
    printf '%s\n' "$TASK_RETRY_BACKOFF_LINES"
  }

  task_needs() {
    _nixfied_task_load_cache "$1" || return 1
    printf '%s\n' "$TASK_NEEDS_LINES"
  }

  task_soft_needs() {
    _nixfied_task_load_cache "$1" || return 1
    printf '%s\n' "$TASK_SOFT_NEEDS_LINES"
  }

  task_hook_count() {
    _nixfied_task_load_cache "$1" || return 1
    printf '%s' "$TASK_HOOK_COUNT"
  }

  task_hook_ids() {
    _nixfied_task_load_cache "$1" || return 1
    case "$2" in
      pre)
        printf '%s\n' "$TASK_PRE_HOOK_IDS_LINES"
        ;;
      post)
        printf '%s\n' "$TASK_POST_HOOK_IDS_LINES"
        ;;
      *)
        return 1
        ;;
    esac
  }

  task_hook_command() {
    _nixfied_task_load_hook_cache "$1" "$2" "$3" || return 1
    printf '%s' "$HOOK_COMMAND"
  }

  task_hook_runtime_plan_shell() {
    _nixfied_task_load_hook_cache "$1" "$2" "$3" || return 1
    printf '%s' "$HOOK_RUNTIME_PLAN_SHELL"
  }

  task_hook_runtime_pass_through_env_names() {
    _nixfied_task_load_hook_cache "$1" "$2" "$3" || return 1
    printf '%s\n' "$HOOK_PASS_THROUGH_ENV_NAMES_LINES"
  }
''
