{ pkgs }:
let
  commonRuntimeShell = import ./common-runtime.nix { inherit pkgs; };
  kernelPackage = import ./kernel { inherit pkgs; };
in
''
  ${commonRuntimeShell}

  JQ_BIN=${pkgs.lib.escapeShellArg "${pkgs.jq}/bin/jq"}
  NIXFIED_TASK_HANDOFF_CURRENT_ID=""
  NIXFIED_WORKFLOW_HANDOFF_CURRENT_ID=""
  LOGGING_FILTERED_ARGS=()
  MACHINE_FILTERED_ARGS=()
  MACHINE_RUN_ID_FILE=""
  MACHINE_SUMMARY_FILE=""

  execution_query_task() {
    local task_id="$1"
    "$JQ_BIN" -c -e --arg task_id "$task_id" '.compiled.execution.tasks.byId[$task_id]' "$NIXFIED_MODEL_FILE"
  }

  execution_query_workflow() {
    local workflow_id="$1"
    "$JQ_BIN" -c -e --arg workflow_id "$workflow_id" '.compiled.execution.workflows.byId[$workflow_id]' "$NIXFIED_MODEL_FILE"
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
    execution_query_task "$1" >/dev/null 2>&1
  }

  task_print_help() {
    local task_id="$1"
    "$JQ_BIN" -r --arg task_id "$task_id" '.compiled.execution.tasks.byId[$task_id].help.lines[]?' "$NIXFIED_MODEL_FILE"
  }

  task_runtime_plan_shell() {
    local task_id="$1"
    "$JQ_BIN" -r --arg task_id "$task_id" '.compiled.execution.tasks.byId[$task_id].runtimePlanShell // ""' "$NIXFIED_MODEL_FILE"
  }

  task_retry_backoff_values() {
    local task_id="$1"
    "$JQ_BIN" -r --arg task_id "$task_id" '.compiled.execution.tasks.byId[$task_id].retryBackoffValues[]?' "$NIXFIED_MODEL_FILE"
  }

  task_hook_ids() {
    local task_id="$1"
    local phase="$2"
    local ids_key=""

    case "$phase" in
      pre)
        ids_key="preIds"
        ;;
      post)
        ids_key="postIds"
        ;;
      *)
        echo "ERROR: unsupported task hook phase '$phase'" >&2
        return 1
        ;;
    esac

    "$JQ_BIN" -r --arg task_id "$task_id" --arg ids_key "$ids_key" '.compiled.execution.tasks.byId[$task_id].hooks[$ids_key][]?' "$NIXFIED_MODEL_FILE"
  }

  task_hook_use() {
    local task_id="$1"
    local phase="$2"
    local hook_id="$3"
    local hook_json=""

    hook_json="$(
      "$JQ_BIN" -c -e --arg task_id "$task_id" --arg phase "$phase" --arg hook_id "$hook_id" '
        .compiled.execution.tasks.byId[$task_id].hooks[$phase][$hook_id]
      ' "$NIXFIED_MODEL_FILE"
    )" || {
      echo "ERROR: unknown task hook '$task_id:$phase:$hook_id'" >&2
      return 1
    }

    NIXFIED_TASK_HOOK_COMMAND="$("$JQ_BIN" -r '.command // ""' <<< "$hook_json")"
    NIXFIED_TASK_HOOK_RUNTIME_PLAN_SHELL="$("$JQ_BIN" -r '.runtimePlanShell // ""' <<< "$hook_json")"
  }

  task_handoff_use() {
    local task_id="$1"
    local task_json=""

    if [ "$NIXFIED_TASK_HANDOFF_CURRENT_ID" = "$task_id" ]; then
      return 0
    fi

    task_json="$(
      "$JQ_BIN" -c -e --arg task_id "$task_id" '
        .compiled.execution.tasks.byId[$task_id]
      ' "$NIXFIED_MODEL_FILE"
    )" || {
      echo "ERROR: unknown task '$task_id'" >&2
      return 1
    }

    NIXFIED_TASK_ID="$task_id"
    NIXFIED_TASK_RUNNER_TYPE="$("$JQ_BIN" -r '.runner.type // "shell"' <<< "$task_json")"
    NIXFIED_TASK_RUNNER_COMMAND="$("$JQ_BIN" -r '.runner.command // ""' <<< "$task_json")"
    NIXFIED_TASK_RUNNER_PACKAGE="$("$JQ_BIN" -r '.runner.package // ""' <<< "$task_json")"
    NIXFIED_TASK_RUNNER_WORKFLOW_ID="$("$JQ_BIN" -r '.runner.workflowId // ""' <<< "$task_json")"
    NIXFIED_TASK_MAX_ATTEMPTS="$("$JQ_BIN" -r '(.maxAttempts // 1) | tostring' <<< "$task_json")"
    NIXFIED_TASK_HOOK_COUNT="$("$JQ_BIN" -r '(.hooks.count // 0) | tostring' <<< "$task_json")"
    NIXFIED_TASK_PRODUCES_JSON="$("$JQ_BIN" -c '.produces // {}' <<< "$task_json")"
    NIXFIED_TASK_HANDOFF_CURRENT_ID="$task_id"
  }

  workflow_mode_override_from_args() {
    local workflow_id="$1"
    shift
    local parse_options=1
    local arg=""
    local mode_override=""

    while [ "$#" -gt 0 ]; do
      arg="$1"
      shift

      if [ "$parse_options" -eq 0 ]; then
        continue
      fi

      case "$arg" in
        --mode)
          if [ "$#" -gt 0 ]; then
            mode_override="$1"
            shift
          else
            break
          fi
          ;;
        --mode=*)
          mode_override="''${arg#--mode=}"
          ;;
        --summary|--exclude-services|--exclude-services=*)
          if [ "$arg" = "--exclude-services" ] && [ "$#" -gt 0 ]; then
            shift
          fi
          ;;
        --)
          parse_options=0
          ;;
        --*)
          if workflow_resolve_mode_id "$workflow_id" "''${arg#--}" >/dev/null 2>&1; then
            mode_override="''${arg#--}"
          fi
          ;;
      esac
    done

    printf '%s' "$mode_override"
  }

  workflow_resolve_mode_id() {
    local workflow_id="$1"
    local mode_override="''${2:-}"

    ${kernelPackage}/bin/nixfied-kernel workflow resolve-mode \
      "$NIXFIED_MODEL_FILE" \
      "$workflow_id" \
      "$mode_override"
  }

  workflow_resolved_id_from_args_lenient() {
    local workflow_id="$1"
    shift
    local mode_override=""

    mode_override="$(workflow_mode_override_from_args "$workflow_id" "$@")"
    if [ -z "$mode_override" ]; then
      printf '%s' "$workflow_id"
      return 0
    fi

    workflow_resolve_mode_id "$workflow_id" "$mode_override" 2>/dev/null || printf '%s' "$workflow_id"
  }

  workflow_resolved_id_from_args_strict() {
    local workflow_id="$1"
    shift
    local mode_override=""

    mode_override="$(workflow_mode_override_from_args "$workflow_id" "$@")"
    if [ -z "$mode_override" ]; then
      printf '%s' "$workflow_id"
      return 0
    fi

    workflow_resolve_mode_id "$workflow_id" "$mode_override"
  }

  workflow_handoff_use() {
    local workflow_id="$1"
    local workflow_json=""

    if [ "$NIXFIED_WORKFLOW_HANDOFF_CURRENT_ID" = "$workflow_id" ]; then
      return 0
    fi

    workflow_json="$(
      "$JQ_BIN" -c -e --arg workflow_id "$workflow_id" '
        .compiled.execution.workflows.byId[$workflow_id]
      ' "$NIXFIED_MODEL_FILE"
    )" || {
      echo "ERROR: unknown workflow '$workflow_id'" >&2
      return 1
    }

    NIXFIED_WORKFLOW_ID="$workflow_id"
    NIXFIED_WORKFLOW_MODE_NAME="$("$JQ_BIN" -r '.mode // "custom"' <<< "$workflow_json")"
    NIXFIED_WORKFLOW_EPHEMERAL_FLAG="$("$JQ_BIN" -r 'if (.ephemeralEnabled // false) then "1" else "0" end' <<< "$workflow_json")"
    NIXFIED_WORKFLOW_LOGGING_LEVEL_DEFAULT="$("$JQ_BIN" -r '.logging.levelDefault // ""' <<< "$workflow_json")"
    NIXFIED_WORKFLOW_LOGGING_OUTPUT_DEFAULT="$("$JQ_BIN" -r '.logging.outputDefault // ""' <<< "$workflow_json")"
    NIXFIED_WORKFLOW_PARALLEL_ENABLED="$("$JQ_BIN" -r 'if (.parallelEnabled // false) then "true" else "false" end' <<< "$workflow_json")"
    NIXFIED_WORKFLOW_MAX_WORKERS="$("$JQ_BIN" -r '(.maxWorkers // 1) | tostring' <<< "$workflow_json")"
    NIXFIED_WORKFLOW_WRITE_SUMMARY="$("$JQ_BIN" -r 'if (.writeSummary // false) then "true" else "false" end' <<< "$workflow_json")"
    NIXFIED_WORKFLOW_ARTIFACTS_ROOT="$("$JQ_BIN" -r '.artifactsRoot // ""' <<< "$workflow_json")"
    NIXFIED_WORKFLOW_HANDOFF_CURRENT_ID="$workflow_id"
  }

  workflow_handoff_use_resolved() {
    local workflow_id="$1"
    shift
    local resolved_workflow_id=""

    resolved_workflow_id="$(workflow_resolved_id_from_args_strict "$workflow_id" "$@")" || return 1
    workflow_handoff_use "$resolved_workflow_id"
  }

  workflow_id_exists() {
    execution_query_workflow "$1" >/dev/null 2>&1
  }

  merge_service_csvs() {
    local first_csv="$1"
    local second_csv="$2"
    local combined_csv=""

    if [ -n "$first_csv" ] && [ -n "$second_csv" ]; then
      combined_csv="$first_csv,$second_csv"
    else
      combined_csv="$first_csv$second_csv"
    fi

    "$JQ_BIN" -rn --arg csv "$combined_csv" '
      ($csv | split(",") | map(gsub("^\\s+|\\s+$"; "") | select(. != "")) | unique) | join(",")
    '
  }

  task_selected_services_csv() {
    local task_id="$1"
    shift
    local base_csv=""
    local workflow_id=""
    local workflow_csv=""
    local resolved_csv=""

    base_csv="$("$JQ_BIN" -r --arg task_id "$task_id" '.compiled.execution.tasks.byId[$task_id].baseClosureServicesCsv // ""' "$NIXFIED_MODEL_FILE")"
    task_handoff_use "$task_id" || return 1
    workflow_id="''${NIXFIED_TASK_RUNNER_WORKFLOW_ID:-}"

    if [ -n "$workflow_id" ]; then
      workflow_id="$(workflow_resolved_id_from_args_lenient "$workflow_id" "$@")"
      workflow_csv="$("$JQ_BIN" -r --arg workflow_id "$workflow_id" '.compiled.execution.workflows.byId[$workflow_id].unitClosureSelectedServices // [] | join(",")' "$NIXFIED_MODEL_FILE")"
      resolved_csv="$(merge_service_csvs "$base_csv" "$workflow_csv")"
    else
      resolved_csv="$base_csv"
    fi

    filter_excluded_services_csv "$resolved_csv"
  }

  workflow_selected_services_csv() {
    local workflow_id="$1"
    shift
    local resolved_workflow_id=""
    local resolved_csv=""

    resolved_workflow_id="$(workflow_resolved_id_from_args_lenient "$workflow_id" "$@")"
    resolved_csv="$("$JQ_BIN" -r --arg workflow_id "$resolved_workflow_id" '.compiled.execution.workflows.byId[$workflow_id].unitClosureSelectedServices // [] | join(",")' "$NIXFIED_MODEL_FILE")"
    filter_excluded_services_csv "$resolved_csv"
  }

  service_set_name() {
    local service_set_id="$1"
    "$JQ_BIN" -r --arg service_set_id "$service_set_id" '.compiled.execution.serviceSets.byId[$service_set_id].name // ""' "$NIXFIED_MODEL_FILE"
  }

  service_set_all_services_csv() {
    local service_set_id="$1"
    "$JQ_BIN" -r --arg service_set_id "$service_set_id" '.compiled.execution.serviceSets.byId[$service_set_id].allServices // [] | join(",")' "$NIXFIED_MODEL_FILE"
  }

  task_validate_args() {
    local task_id="$1"
    shift

    ${kernelPackage}/bin/nixfied-kernel task validate-args \
      "$NIXFIED_MODEL_FILE" \
      "$task_id" \
      -- "$@"
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
        --exclude-services)
          if [ "$#" -lt 1 ]; then
            echo "ERROR: --exclude-services requires a value"
            return 2
          fi
          export NIXFIED_EXCLUDED_SERVICES_CSV="$1"
          shift
          ;;
        --exclude-services=*)
          export NIXFIED_EXCLUDED_SERVICES_CSV="''${arg#--exclude-services=}"
          ;;
        *)
          MACHINE_FILTERED_ARGS+=("$arg")
          ;;
      esac
    done
  }
''
