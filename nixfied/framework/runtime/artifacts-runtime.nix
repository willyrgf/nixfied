''
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

  resolve_run_artifacts_dir_impl() {
    local run_id="$1"
    local workflow_id="$2"
    local absolutize_base="$3"
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

    if [ "$absolutize_base" = "1" ]; then
      base_dir="$(absolutize_artifacts_base_dir "$base_dir")"
    fi

    normalize_run_artifacts_dir "$base_dir" "$run_id" "$attempt_id"
  }

  ensure_executor_run_artifacts_dir() {
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

    artifacts_dir="$(resolve_run_artifacts_dir_impl "$run_id" "$workflow_id" "0")" || return $?
    export CI_ARTIFACTS_DIR="$artifacts_dir"
    if ! mkdir -p "$CI_ARTIFACTS_DIR"; then
      echo "ERROR: failed to create artifacts directory '$CI_ARTIFACTS_DIR'"
      return 1
    fi
  }

  ensure_orchestrator_artifacts_root() {
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
        artifacts_dir="$(resolve_run_artifacts_dir_impl "$run_id" "$workflow_id" "1")" || return $?
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
    artifacts_dir="$(resolve_run_artifacts_dir_impl "$run_id" "$workflow_id" "1")" || return $?
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
