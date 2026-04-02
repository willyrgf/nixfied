{ pkgs }:
let
  commonRuntimeShell = import ./common-runtime.nix { inherit pkgs; };
  kernelPackage = import ./kernel { inherit pkgs; };
in
''
  ${commonRuntimeShell}

  NIXFIED_TASK_HANDOFF_CURRENT_ID=""
  NIXFIED_WORKFLOW_HANDOFF_CURRENT_ID=""
  LOGGING_FILTERED_ARGS=()
  MACHINE_FILTERED_ARGS=()
  MACHINE_RUN_ID_FILE=""
  MACHINE_SUMMARY_FILE=""

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

  eval_kernel_exports() {
    local export_text="$1"

    if [ -z "$export_text" ]; then
      echo "ERROR: kernel export stream is empty"
      return 1
    fi

    eval "$export_text"
  }

  task_export_text() {
    ${kernelPackage}/bin/nixfied-kernel task export "$NIXFIED_MODEL_FILE" "$1"
  }

  workflow_export_text() {
    ${kernelPackage}/bin/nixfied-kernel workflow export "$NIXFIED_MODEL_FILE" "$1"
  }

  workflow_export_resolved_text() {
    local workflow_id="$1"
    shift

    ${kernelPackage}/bin/nixfied-kernel workflow export-resolved \
      "$NIXFIED_MODEL_FILE" \
      "$workflow_id" \
      -- "$@"
  }

  task_descriptor_exists() {
    task_export_text "$1" >/dev/null 2>&1
  }

  task_print_help() {
    ${kernelPackage}/bin/nixfied-kernel task help "$NIXFIED_MODEL_FILE" "$1"
  }

  task_runtime_plan_shell() {
    ${kernelPackage}/bin/nixfied-kernel task runtime-plan "$NIXFIED_MODEL_FILE" "$1"
  }

  task_retry_backoff_values() {
    ${kernelPackage}/bin/nixfied-kernel task retry-backoff-values "$NIXFIED_MODEL_FILE" "$1"
  }

  task_selected_services_csv() {
    local task_id="$1"
    shift

    ${kernelPackage}/bin/nixfied-kernel task selected-services-csv \
      "$NIXFIED_MODEL_FILE" \
      "$task_id" \
      -- "$@"
  }

  task_hook_ids() {
    ${kernelPackage}/bin/nixfied-kernel task hook-ids "$NIXFIED_MODEL_FILE" "$1" "$2"
  }

  task_hook_use() {
    local task_id="$1"
    local phase="$2"
    local hook_id="$3"
    local export_text=""

    export_text="$(${kernelPackage}/bin/nixfied-kernel task hook-export "$NIXFIED_MODEL_FILE" "$task_id" "$phase" "$hook_id")" || return 1
    eval_kernel_exports "$export_text"
  }

  task_handoff_use() {
    local task_id="$1"
    local export_text=""

    if [ "$NIXFIED_TASK_HANDOFF_CURRENT_ID" = "$task_id" ]; then
      return 0
    fi

    export_text="$(task_export_text "$task_id")" || return 1
    eval_kernel_exports "$export_text" || return 1
    NIXFIED_TASK_HANDOFF_CURRENT_ID="$task_id"
  }

  workflow_handoff_use() {
    local workflow_id="$1"
    local export_text=""

    if [ "$NIXFIED_WORKFLOW_HANDOFF_CURRENT_ID" = "$workflow_id" ]; then
      return 0
    fi

    export_text="$(workflow_export_text "$workflow_id")" || return 1
    eval_kernel_exports "$export_text" || return 1
    NIXFIED_WORKFLOW_HANDOFF_CURRENT_ID="$workflow_id"
  }

  workflow_handoff_use_resolved() {
    local workflow_id="$1"
    shift
    local export_text=""

    export_text="$(workflow_export_resolved_text "$workflow_id" "$@")" || return 1
    eval_kernel_exports "$export_text" || return 1
    NIXFIED_WORKFLOW_HANDOFF_CURRENT_ID="$NIXFIED_WORKFLOW_ID"
  }

  workflow_id_exists() {
    workflow_resolve_mode_id "$1" "" >/dev/null 2>&1
  }

  workflow_selected_services_csv() {
    local workflow_id="$1"
    shift

    ${kernelPackage}/bin/nixfied-kernel workflow selected-services-csv \
      "$NIXFIED_MODEL_FILE" \
      "$workflow_id" \
      -- "$@"
  }

  task_validate_args() {
    local task_id="$1"
    shift

    ${kernelPackage}/bin/nixfied-kernel task validate-args \
      "$NIXFIED_MODEL_FILE" \
      "$task_id" \
      -- "$@"
  }

  workflow_resolve_mode_id() {
    local workflow_id="$1"
    local mode_override="''${2:-}"

    ${kernelPackage}/bin/nixfied-kernel workflow resolve-mode \
      "$NIXFIED_MODEL_FILE" \
      "$workflow_id" \
      "$mode_override"
  }

  extract_logging_override_args() {
    local parse_options=1
    local arg=""
    local value=""
    local resolved_log_level="''${NIXFIED_CLI_LOG_LEVEL_OVERRIDE:-}"
    local resolved_output_mode="''${NIXFIED_CLI_OUTPUT_MODE_OVERRIDE:-}"

    LOGGING_FILTERED_ARGS=()

    while [ "$#" -gt 0 ]; do
      arg="$1"
      shift

      if [ "$parse_options" -eq 0 ]; then
        LOGGING_FILTERED_ARGS+=("$arg")
        continue
      fi

      case "$arg" in
        --)
          parse_options=0
          LOGGING_FILTERED_ARGS+=("--")
          ;;
        --log-level)
          if [ "$#" -lt 1 ]; then
            echo "ERROR: --log-level requires a value"
            return 2
          fi
          value="$1"
          shift
          if ! valid_log_level "$value"; then
            echo "ERROR: invalid --log-level '$value' (expected: error|warn|info|debug|trace)"
            return 2
          fi
          resolved_log_level="$value"
          ;;
        --log-level=*)
          value="''${arg#--log-level=}"
          if ! valid_log_level "$value"; then
            echo "ERROR: invalid --log-level '$value' (expected: error|warn|info|debug|trace)"
            return 2
          fi
          resolved_log_level="$value"
          ;;
        --output-mode)
          if [ "$#" -lt 1 ]; then
            echo "ERROR: --output-mode requires a value"
            return 2
          fi
          value="$1"
          shift
          if ! valid_output_mode "$value"; then
            echo "ERROR: invalid --output-mode '$value' (expected: stdout|logs|both)"
            return 2
          fi
          resolved_output_mode="$value"
          ;;
        --output-mode=*)
          value="''${arg#--output-mode=}"
          if ! valid_output_mode "$value"; then
            echo "ERROR: invalid --output-mode '$value' (expected: stdout|logs|both)"
            return 2
          fi
          resolved_output_mode="$value"
          ;;
        *)
          LOGGING_FILTERED_ARGS+=("$arg")
          ;;
      esac
    done

    if [ -n "$resolved_log_level" ]; then
      export NIXFIED_CLI_LOG_LEVEL_OVERRIDE="$resolved_log_level"
    else
      unset NIXFIED_CLI_LOG_LEVEL_OVERRIDE || true
    fi

    if [ -n "$resolved_output_mode" ]; then
      export NIXFIED_CLI_OUTPUT_MODE_OVERRIDE="$resolved_output_mode"
    else
      unset NIXFIED_CLI_OUTPUT_MODE_OVERRIDE || true
    fi
  }

  extract_machine_output_args() {
    local parse_options=1
    local arg=""
    local value=""

    MACHINE_FILTERED_ARGS=()
    MACHINE_RUN_ID_FILE="''${NIXFIED_RUN_ID_FILE_OVERRIDE:-}"
    MACHINE_SUMMARY_FILE="''${NIXFIED_SUMMARY_FILE_OVERRIDE:-}"

    while [ "$#" -gt 0 ]; do
      arg="$1"
      shift

      if [ "$parse_options" -eq 0 ]; then
        MACHINE_FILTERED_ARGS+=("$arg")
        continue
      fi

      case "$arg" in
        --)
          parse_options=0
          MACHINE_FILTERED_ARGS+=("--")
          ;;
        --run-id-file)
          if [ "$#" -lt 1 ]; then
            echo "ERROR: --run-id-file requires a value"
            return 2
          fi
          value="$1"
          shift
          if [ -z "$value" ]; then
            echo "ERROR: --run-id-file requires a non-empty value"
            return 2
          fi
          MACHINE_RUN_ID_FILE="$value"
          ;;
        --run-id-file=*)
          value="''${arg#--run-id-file=}"
          if [ -z "$value" ]; then
            echo "ERROR: --run-id-file requires a non-empty value"
            return 2
          fi
          MACHINE_RUN_ID_FILE="$value"
          ;;
        --summary-file)
          if [ "$#" -lt 1 ]; then
            echo "ERROR: --summary-file requires a value"
            return 2
          fi
          value="$1"
          shift
          if [ -z "$value" ]; then
            echo "ERROR: --summary-file requires a non-empty value"
            return 2
          fi
          MACHINE_SUMMARY_FILE="$value"
          ;;
        --summary-file=*)
          value="''${arg#--summary-file=}"
          if [ -z "$value" ]; then
            echo "ERROR: --summary-file requires a non-empty value"
            return 2
          fi
          MACHINE_SUMMARY_FILE="$value"
          ;;
        *)
          MACHINE_FILTERED_ARGS+=("$arg")
          ;;
      esac
    done
  }
''
