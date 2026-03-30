{ pkgs }:
let
  commonRuntimeShell = import ./common-runtime.nix { inherit pkgs; };
  kernelPackage = import ./kernel { inherit pkgs; };
in
''
  ${commonRuntimeShell}

  validate_log_level_value() {
    local value="$1"
    if valid_log_level "$value"; then
      return 0
    fi
    echo "ERROR: invalid --log-level '$value' (expected: error|warn|info|debug|trace)"
    return 2
  }

  validate_output_mode_value() {
    local value="$1"
    if valid_output_mode "$value"; then
      return 0
    fi
    echo "ERROR: invalid --output-mode '$value' (expected: stdout|logs|both)"
    return 2
  }

  split_process_mode() {
    PROCESS_MODE="fg"
    FORWARD_ARGS=()
    MACHINE_RUN_ID_FILE="''${NIXFIED_RUN_ID_FILE_OVERRIDE:-}"
    MACHINE_SUMMARY_FILE="''${NIXFIED_SUMMARY_FILE_OVERRIDE:-}"

    local parse_opts=1
    local arg

    while [ "$#" -gt 0 ]; do
      arg="$1"
      shift

      if [ "$parse_opts" -eq 1 ]; then
        case "$arg" in
          --bg)
            PROCESS_MODE="bg"
            continue
            ;;
          --fg)
            PROCESS_MODE="fg"
            continue
            ;;
          --run-id-file)
            if [ "$#" -lt 1 ]; then
              echo "ERROR: --run-id-file requires a value"
              return 2
            fi
            MACHINE_RUN_ID_FILE="$1"
            shift
            continue
            ;;
          --run-id-file=*)
            MACHINE_RUN_ID_FILE="''${arg#--run-id-file=}"
            continue
            ;;
          --summary-file)
            if [ "$#" -lt 1 ]; then
              echo "ERROR: --summary-file requires a value"
              return 2
            fi
            MACHINE_SUMMARY_FILE="$1"
            shift
            continue
            ;;
          --summary-file=*)
            MACHINE_SUMMARY_FILE="''${arg#--summary-file=}"
            continue
            ;;
          --)
            parse_opts=0
            FORWARD_ARGS+=("--")
            continue
            ;;
        esac
      fi

      FORWARD_ARGS+=("$arg")
    done

    if [ -n "$MACHINE_RUN_ID_FILE" ]; then
      export NIXFIED_RUN_ID_FILE_OVERRIDE="$MACHINE_RUN_ID_FILE"
    else
      unset NIXFIED_RUN_ID_FILE_OVERRIDE || true
    fi

    if [ -n "$MACHINE_SUMMARY_FILE" ]; then
      export NIXFIED_SUMMARY_FILE_OVERRIDE="$MACHINE_SUMMARY_FILE"
    else
      unset NIXFIED_SUMMARY_FILE_OVERRIDE || true
    fi
  }

  run_file_state() {
    local run_file="$1"
    ${kernelPackage}/bin/nixfied-kernel run-record read "$run_file" state
  }

  run_file_pid() {
    local run_file="$1"
    ${kernelPackage}/bin/nixfied-kernel run-record read "$run_file" pid
  }

  run_file_pgid() {
    local run_file="$1"
    ${kernelPackage}/bin/nixfied-kernel run-record read "$run_file" pgid
  }

  run_file_attempt_id() {
    local run_file="$1"
    ${kernelPackage}/bin/nixfied-kernel run-record read "$run_file" attempt_id
  }

  run_file_command() {
    local run_file="$1"
    ${kernelPackage}/bin/nixfied-kernel run-record read "$run_file" command
  }

  run_file_process_mode() {
    local run_file="$1"
    ${kernelPackage}/bin/nixfied-kernel run-record read "$run_file" process_mode
  }

  validate_workflow_args() {
    local workflow_id="$1"
    shift

    local parse_opts=1
    local arg
    local mode_value
    local shorthand_mode

    while [ "$#" -gt 0 ]; do
      arg="$1"
      shift

      if [ "$parse_opts" -eq 0 ]; then
        continue
      fi

      case "$arg" in
        --)
          parse_opts=0
          ;;
        --mode)
          if [ "$#" -lt 1 ]; then
            echo "ERROR: --mode requires a value"
            return 2
          fi
          mode_value="$1"
          shift
          workflow_resolve_mode_id "$workflow_id" "$mode_value" >/dev/null || return $?
          ;;
        --mode=*)
          mode_value="''${arg#--mode=}"
          workflow_resolve_mode_id "$workflow_id" "$mode_value" >/dev/null || return $?
          ;;
        --summary)
          ;;
        --run-id-file)
          if [ "$#" -lt 1 ]; then
            echo "ERROR: --run-id-file requires a value"
            return 2
          fi
          shift
          ;;
        --run-id-file=*)
          ;;
        --summary-file)
          if [ "$#" -lt 1 ]; then
            echo "ERROR: --summary-file requires a value"
            return 2
          fi
          shift
          ;;
        --summary-file=*)
          ;;
        --log-level)
          if [ "$#" -lt 1 ]; then
            echo "ERROR: --log-level requires a value"
            return 2
          fi
          validate_log_level_value "$1" || return $?
          shift
          ;;
        --log-level=*)
          validate_log_level_value "''${arg#--log-level=}" || return $?
          ;;
        --output-mode)
          if [ "$#" -lt 1 ]; then
            echo "ERROR: --output-mode requires a value"
            return 2
          fi
          validate_output_mode_value "$1" || return $?
          shift
          ;;
        --output-mode=*)
          validate_output_mode_value "''${arg#--output-mode=}" || return $?
          ;;
        --*)
          shorthand_mode="''${arg#--}"
          if workflow_simple_shorthand_exists_for_family "$workflow_id" "$shorthand_mode"; then
            continue
          fi
          echo "ERROR: unknown option '$arg' for workflow '$workflow_id'"
          return 2
          ;;
        -*)
          echo "ERROR: unknown option '$arg' for workflow '$workflow_id'"
          return 2
          ;;
        *)
          ;;
      esac
    done
  }

  validate_typed_task_args() {
    local task_id="$1"
    shift
    local validate_output=""
    local validate_rc=0

    if ! task_descriptor_exists "$task_id"; then
      echo "ERROR: unknown task '$task_id'"
      return 2
    fi

    if validate_output="$(task_validate_args "$task_id" "$@" 2>&1)"; then
      return 0
    fi
    validate_rc="$?"

    case "$validate_output" in
      "ERROR: unknown option "*)
        validate_output="$validate_output for task '$task_id'"
        ;;
    esac

    if [ -n "$validate_output" ]; then
      printf '%s\n' "$validate_output"
    fi
    return "$validate_rc"
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

  invocation_root_dir() {
    local caller_pwd="''${NIXFIED_CALLER_PWD:-}"

    if [ -n "$caller_pwd" ] && [ -d "$caller_pwd" ]; then
      (
        cd "$caller_pwd"
        pwd -P
      )
      return 0
    fi

    pwd -P
  }

  absolutize_artifacts_base_dir() {
    local base_dir="$1"
    local invocation_root

    case "$base_dir" in
      "")
        printf '%s' ""
        ;;
      /*)
        printf '%s' "$base_dir"
        ;;
      *)
        invocation_root="$(invocation_root_dir)"
        if [ "$base_dir" = "." ]; then
          printf '%s' "$invocation_root"
        else
          printf '%s/%s' "$invocation_root" "$base_dir"
        fi
        ;;
    esac
  }

  resolve_run_artifacts_dir() {
    local run_id="$1"
    local workflow_id="$2"
    local attempt_id="''${NIXFIED_ATTEMPT_ID:-''${NIXFIED_ORCHESTRATOR_ATTEMPT_ID:-}}"
    local configured_root
    local caller_root="''${CI_ARTIFACTS_ROOT:-}"
    local caller_dir="''${CI_ARTIFACTS_DIR:-}"
    local base_dir=""

    configured_root="$(workflow_artifacts_root "$workflow_id")"

    if [ -n "$caller_root" ] && [ -n "$caller_dir" ]; then
      echo "ERROR: CI_ARTIFACTS_ROOT and CI_ARTIFACTS_DIR cannot both be set"
      return 2
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

    base_dir="$(absolutize_artifacts_base_dir "$base_dir")"

    normalize_run_artifacts_dir "$base_dir" "$run_id" "$attempt_id"
  }

  resolve_task_workflow_ref() {
    local task_id="$1"
    local runner_type

    if ! task_descriptor_exists "$task_id"; then
      printf '%s' ""
      return
    fi

    task_handoff_use "$task_id" >/dev/null 2>&1 || {
      printf '%s' ""
      return
    }
    runner_type="$NIXFIED_TASK_RUNNER_TYPE"
    if [ "$runner_type" = "workflowRef" ]; then
      printf '%s' "$NIXFIED_TASK_RUNNER_WORKFLOW_ID"
      return
    fi

    printf '%s' ""
  }

  ensure_artifacts_root() {
    local run_id="$1"
    local ephemeral_enabled="$2"
    local workflow_id="$3"

    local artifacts_dir
    local runtime_scope_dir
    local caller_root="''${CI_ARTIFACTS_ROOT:-}"
    local caller_dir="''${CI_ARTIFACTS_DIR:-}"

    if [ "$ephemeral_enabled" = "1" ]; then
      export NIXFIED_EXECUTION_EPHEMERAL=1
      unset NIXFIED_RUNTIME_DIR_SCOPE_OVERRIDE || true

      if [ -n "$caller_root" ] || [ -n "$caller_dir" ]; then
        artifacts_dir="$(resolve_run_artifacts_dir "$run_id" "$workflow_id")" || return $?
        export CI_ARTIFACTS_DIR="$artifacts_dir"
        if ! mkdir -p "$CI_ARTIFACTS_DIR"; then
          echo "ERROR: failed to prepare CI_ARTIFACTS_DIR '$CI_ARTIFACTS_DIR'"
          return 3
        fi
      else
        unset CI_ARTIFACTS_DIR || true
      fi

      unset CI_ARTIFACTS_ROOT || true
      return 0
    fi

    export NIXFIED_EXECUTION_EPHEMERAL=0
    artifacts_dir="$(resolve_run_artifacts_dir "$run_id" "$workflow_id")" || return $?
    export CI_ARTIFACTS_DIR="$artifacts_dir"
    runtime_scope_dir="$artifacts_dir/.runtime"
    export NIXFIED_RUNTIME_DIR_SCOPE_OVERRIDE="$runtime_scope_dir"

    if [ -n "$CI_ARTIFACTS_DIR" ]; then
      if ! mkdir -p "$CI_ARTIFACTS_DIR"; then
        echo "ERROR: failed to prepare CI_ARTIFACTS_DIR '$CI_ARTIFACTS_DIR'"
        return 3
      fi
    fi
    if [ -n "$NIXFIED_RUNTIME_DIR_SCOPE_OVERRIDE" ]; then
      if ! mkdir -p "$NIXFIED_RUNTIME_DIR_SCOPE_OVERRIDE"; then
        echo "ERROR: failed to prepare NIXFIED_RUNTIME_DIR_SCOPE_OVERRIDE '$NIXFIED_RUNTIME_DIR_SCOPE_OVERRIDE'"
        return 3
      fi
    fi
  }
''
