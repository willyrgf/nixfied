{ pkgs }:
let
  commonRuntimeShell = import ./common-runtime.nix { inherit pkgs; };
  kernelPackage = import ./kernel { inherit pkgs; };
in
''
  ${commonRuntimeShell}

  NIXFIED_TASK_HANDOFF_CURRENT_ID=""
  NIXFIED_TASK_HANDOFF_CURRENT_DIR=""
  NIXFIED_WORKFLOW_HANDOFF_CURRENT_ID=""
  NIXFIED_WORKFLOW_HANDOFF_CURRENT_DIR=""
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

  runtime_handoff_root() {
    if [ -z "''${NIXFIED_RUNTIME_HANDOFF_ROOT:-}" ]; then
      NIXFIED_RUNTIME_HANDOFF_ROOT="$(mktemp -d "''${TMPDIR:-/tmp}/nixfied-runtime-handoff.XXXXXX")" || {
        echo "ERROR: failed to create runtime handoff cache"
        return 1
      }
      export NIXFIED_RUNTIME_HANDOFF_ROOT
    fi

    printf '%s' "$NIXFIED_RUNTIME_HANDOFF_ROOT"
  }

  runtime_handoff_key() {
    local key="$1"
    key="''${key//\//_}"
    key="''${key//:/_}"
    printf '%s' "$key"
  }

  task_handoff_dir() {
    local task_id="$1"
    local root=""
    root="$(runtime_handoff_root)" || return 1
    printf '%s/tasks/%s' "$root" "$(runtime_handoff_key "$task_id")"
  }

  workflow_handoff_dir() {
    local workflow_id="$1"
    local root=""
    root="$(runtime_handoff_root)" || return 1
    printf '%s/workflows/%s' "$root" "$(runtime_handoff_key "$workflow_id")"
  }

  task_handoff_ensure() {
    local task_id="$1"
    local dir=""
    local ready_file=""

    dir="$(task_handoff_dir "$task_id")" || return 1
    ready_file="$dir/.ready"
    if [ ! -f "$ready_file" ]; then
      mkdir -p "$dir" || return 1
      if ! ${kernelPackage}/bin/nixfied-kernel task handoff "$NIXFIED_MODEL_FILE" "$task_id" "$dir"; then
        return 1
      fi
      : > "$ready_file"
    fi

    printf '%s' "$dir"
  }

  workflow_handoff_ensure() {
    local workflow_id="$1"
    local dir=""
    local ready_file=""

    dir="$(workflow_handoff_dir "$workflow_id")" || return 1
    ready_file="$dir/.ready"
    if [ ! -f "$ready_file" ]; then
      mkdir -p "$dir" || return 1
      if ! ${kernelPackage}/bin/nixfied-kernel workflow handoff "$NIXFIED_MODEL_FILE" "$workflow_id" "$dir"; then
        return 1
      fi
      : > "$ready_file"
    fi

    printf '%s' "$dir"
  }

  task_handoff_use() {
    local task_id="$1"
    local dir=""

    if [ "$NIXFIED_TASK_HANDOFF_CURRENT_ID" = "$task_id" ] && [ -n "$NIXFIED_TASK_HANDOFF_CURRENT_DIR" ]; then
      return 0
    fi

    dir="$(task_handoff_ensure "$task_id")" || return 1
    . "$dir/exports.sh"
    NIXFIED_TASK_HANDOFF_CURRENT_ID="$task_id"
    NIXFIED_TASK_HANDOFF_CURRENT_DIR="$dir"
  }

  workflow_handoff_use() {
    local workflow_id="$1"
    local dir=""

    if [ "$NIXFIED_WORKFLOW_HANDOFF_CURRENT_ID" = "$workflow_id" ] && [ -n "$NIXFIED_WORKFLOW_HANDOFF_CURRENT_DIR" ]; then
      return 0
    fi

    dir="$(workflow_handoff_ensure "$workflow_id")" || return 1
    . "$dir/exports.sh"
    NIXFIED_WORKFLOW_HANDOFF_CURRENT_ID="$workflow_id"
    NIXFIED_WORKFLOW_HANDOFF_CURRENT_DIR="$dir"
  }

  task_hook_handoff_dir() {
    local task_id="$1"
    local phase="$2"
    local hook_id="$3"
    local dir=""

    dir="$(task_handoff_ensure "$task_id")" || return 1
    printf '%s/hooks/%s/%s' "$dir" "$phase" "$hook_id"
  }

  task_descriptor_exists() {
    task_handoff_ensure "$1" >/dev/null 2>&1
  }

  workflow_id_exists() {
    workflow_resolve_mode_id "$1" "" >/dev/null 2>&1
  }

  task_print_help() {
    local dir=""
    dir="$(task_handoff_ensure "$1")" || return 1
    cat "$dir/help.txt"
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

  workflow_simple_shorthand_exists_for_family() {
    local workflow_id="$1"
    local candidate="$2"

    workflow_resolve_mode_id "$workflow_id" "$candidate" >/dev/null 2>&1
  }

  normalize_run_artifacts_dir() {
    local base_dir="$1"
    local run_id="$2"
    local attempt_id="$3"

    if [ -z "$attempt_id" ]; then
      attempt_id="$run_id"
    fi

    case "$base_dir" in
      */"$attempt_id")
        printf '%s' "$base_dir"
        ;;
      */"$run_id")
        printf '%s/%s' "$base_dir" "$attempt_id"
        ;;
      *)
        printf '%s/%s/%s' "$base_dir" "$run_id" "$attempt_id"
        ;;
    esac
  }

  resolve_run_artifacts_dir() {
    local run_id="$1"
    local workflow_id="$2"
    local caller_root="''${CI_ARTIFACTS_ROOT:-}"
    local caller_dir="''${CI_ARTIFACTS_DIR:-}"
    local attempt_id="''${NIXFIED_ATTEMPT_ID:-''${NIXFIED_ORCHESTRATOR_ATTEMPT_ID:-}}"
    local configured_root=""
    local base_dir=""

    if [ -n "$caller_root" ] && [ -n "$caller_dir" ]; then
      echo "ERROR: CI_ARTIFACTS_ROOT and CI_ARTIFACTS_DIR cannot both be set"
      return 2
    fi

    if [ -n "$workflow_id" ]; then
      workflow_handoff_use "$workflow_id" || return 1
      configured_root="$NIXFIED_WORKFLOW_ARTIFACTS_ROOT"
    fi

    if [ -n "$caller_root" ]; then
      base_dir="$caller_root"
    elif [ -n "$caller_dir" ]; then
      base_dir="$caller_dir"
    elif [ -n "$configured_root" ]; then
      base_dir="$configured_root"
    elif [ "$REGISTRY_ROOT_EXPLICIT" = "1" ]; then
      base_dir="$REGISTRY_ROOT/artifacts"
    else
      base_dir="$ARTIFACTS_ROOT_DEFAULT"
    fi

    normalize_run_artifacts_dir "$base_dir" "$run_id" "$attempt_id"
  }

  ensure_run_artifacts_dir() {
    local run_id="$1"
    local workflow_id="$2"
    local managed_by_orchestrator="$3"
    local artifacts_dir

    if [ -n "''${CI_ARTIFACTS_DIR:-}" ] && {
      [ "$managed_by_orchestrator" = "1" ] ||
      [ "''${NIXFIED_WORKFLOW_NESTED:-0}" = "1" ] ||
      [ "''${NIXFIED_EXECUTION_EPHEMERAL:-0}" = "1" ]
    }; then
      mkdir -p "$CI_ARTIFACTS_DIR"
      return 0
    fi

    artifacts_dir="$(resolve_run_artifacts_dir "$run_id" "$workflow_id")" || return $?
    export CI_ARTIFACTS_DIR="$artifacts_dir"
    if ! mkdir -p "$CI_ARTIFACTS_DIR"; then
      echo "ERROR: failed to create artifacts directory '$CI_ARTIFACTS_DIR'"
      return 1
    fi
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
