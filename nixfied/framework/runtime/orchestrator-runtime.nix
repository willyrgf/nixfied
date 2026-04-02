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
          if workflow_resolve_mode_id "$workflow_id" "$shorthand_mode" >/dev/null 2>&1; then
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
''
