# Runtime lifecycle event helpers built on the shared NDJSON registry.
{
  pkgs,
  project ? { },
  loggingPrelude ? null,
}:

let
  projectMeta = project.project or { };
  projectId = projectMeta.id or "project";
  kernelPackage = import ../kernel { inherit pkgs; };
  id = import ./id.nix {
    inherit pkgs project;
  };
  registry = import ../registry/events.nix { inherit pkgs; };
  projectIdUpper =
    let
      replaced = pkgs.lib.replaceStrings [ "-" "." ] [ "_" "_" ] projectId;
    in
    pkgs.lib.strings.toUpper replaced;
  slotVar = projectMeta.slotVar or "NIX_ENV";
  envVar = projectMeta.envVar or "PROJECT_ENV";
  processCfg = project.process or { };
  registryRoot =
    if project ? state && project.state ? policy && project.state.policy ? registryRoot then
      project.state.policy.registryRoot
    else if project ? state && project.state ? registry && project.state.registry ? root then
      project.state.registry.root
    else if project ? state && project.state ? registryRoot then
      project.state.registryRoot
    else
      processCfg.registryRoot or "/tmp/nixfied-runtime/${projectId}/registry";
  baseDirExpr =
    if project ? state && project.state ? policy && project.state.policy ? runtimeBase then
      project.state.policy.runtimeBase
    else
      (project.directories.base or "\${XDG_DATA_HOME:-$HOME/.local/share}/${projectId}");
  ciCfg = project.ci or { };
  artifactsCfg = ciCfg.artifacts or { };
  artifactsRootExpr =
    if project ? state && project.state ? policy && project.state.policy ? artifactsRoot then
      project.state.policy.artifactsRoot
    else
      artifactsCfg.dir or "/tmp/ci-artifacts";
  ephemeralPrefix = "/tmp/${projectId}-ephemeral-";
  resolvedLoggingPrelude =
    if loggingPrelude != null && loggingPrelude != "" then
      loggingPrelude
    else
      (import ./helpers.nix {
        inherit pkgs project;
        hooks = { };
        summaryParser = "";
      }).loggingPrelude;
  registryShell = registry.mkShellLib { };

  sharedPrelude = ''
    ${resolvedLoggingPrelude}

    set -euo pipefail

    REGISTRY_ROOT_DEFAULT="${registryRoot}"
    REGISTRY_ROOT="''${REGISTRY_ROOT:-$REGISTRY_ROOT_DEFAULT}"
    PROJECT_ID="${projectId}"
    BASE_DIR_DEFAULT="${baseDirExpr}"
    CI_ARTIFACTS_BASE_DEFAULT="${artifactsRootExpr}"
    EPHEMERAL_PREFIX="${ephemeralPrefix}"
    SLOT_VAR="${slotVar}"
    ENV_VAR="${envVar}"
    EPHEMERAL_FLAG_VAR="${projectIdUpper}_EPHEMERAL"
    EPHEMERAL_ROOT_VAR="${projectIdUpper}_EPHEMERAL_ROOT"

    ${registryShell}

    normalize_bool() {
      case "''${1:-}" in
        1|true|TRUE|yes|YES|on|ON) echo "true" ;;
        0|false|FALSE|no|NO|off|OFF) echo "false" ;;
        *) echo "null" ;;
      esac
    }

    load_runtime_event_policy_exports() {
      local explicit_reuse="$1"
      local explicit_owner="$2"
      local explicit_discovery="$3"
      local resolved_reuse="$explicit_reuse"
      local resolved_owner="$explicit_owner"
      local resolved_discovery="$explicit_discovery"
      local ephemeral_flag="0"
      local export_file=""

      if [ -z "$resolved_reuse" ]; then
        resolved_reuse="''${SERVICE_REUSE_POLICY:-}"
      fi
      if [ -z "$resolved_owner" ]; then
        resolved_owner="''${SERVICE_OWNER_SCOPE:-}"
      fi
      if [ -z "$resolved_discovery" ]; then
        resolved_discovery="''${SERVICE_DISCOVERY_SCOPE:-}"
      fi
      if [ "''${!EPHEMERAL_FLAG_VAR:-0}" = "1" ]; then
        ephemeral_flag="1"
      fi

      export_file="$(mktemp "''${TMPDIR:-/tmp}/nixfied-runtime-policy.XXXXXX")" || return 1
      if ! ${kernelPackage}/bin/nixfied-kernel service-policy runtime-event \
        "$resolved_reuse" \
        "$resolved_owner" \
        "$resolved_discovery" \
        "$ephemeral_flag" \
        "$export_file" >/dev/null; then
        rm -f "$export_file"
        return 1
      fi
      if ! . "$export_file"; then
        rm -f "$export_file"
        return 1
      fi
      rm -f "$export_file"
    }

    is_numeric_pid() {
      case "''${1:-}" in
        ""|*[!0-9]*) return 1 ;;
        *) return 0 ;;
      esac
    }

    runtime_index_segment() {
      local value="$1"
      if [ -z "$value" ]; then
        printf '%s' "__empty__"
        return 0
      fi
      value="''${value//\//_}"
      value="''${value//$'\n'/_}"
      value="''${value//$'\r'/_}"
      value="''${value//$'\t'/_}"
      printf '%s' "$value"
    }

    runtime_events_index_root() {
      printf '%s/runtime-events' "$REGISTRY_ROOT"
    }

    service_events_root_for() {
      local service_name="$1"
      printf '%s/services/%s' "$(runtime_events_index_root)" "$(runtime_index_segment "$service_name")"
    }

    service_events_index_file_for() {
      local service_name="$1"
      local slot_name="$2"
      local env_name="$3"
      printf '%s/%s/%s/events.tsv' \
        "$(service_events_root_for "$service_name")" \
        "$(runtime_index_segment "$slot_name")" \
        "$(runtime_index_segment "$env_name")"
    }

    slot_events_index_file_for() {
      local slot_name="$1"
      local env_name="$2"
      printf '%s/slots/%s/%s/events.tsv' \
        "$(runtime_events_index_root)" \
        "$(runtime_index_segment "$slot_name")" \
        "$(runtime_index_segment "$env_name")"
    }

    append_index_line_locked() {
      local target_file="$1"
      local line="$2"
      local lock_file="$target_file.lock"
      local lock_fd
      local parent_dir

      parent_dir="$(dirname "$target_file")" || return 1
      mkdir -p "$parent_dir" || return 1
      lock_fd="$(registry_lock_acquire "$lock_file" "runtime-events-index:$target_file" "$REGISTRY_DEFAULT_LOCK_TIMEOUT_SECONDS")" || return 1
      if ! printf '%s\n' "$line" >> "$target_file"; then
        registry_lock_release "$lock_fd" "$lock_file"
        return 1
      fi
      registry_lock_release "$lock_fd" "$lock_file"
    }

    load_runtime_status_exports() {
      local service_name="$1"
      local slot_name="$2"
      local env_name="$3"
      local service_index_file=""
      local slot_index_file=""
      local export_file=""

      service_index_file="$(service_events_index_file_for "$service_name" "$slot_name" "$env_name")"
      slot_index_file="$(slot_events_index_file_for "$slot_name" "$env_name")"
      export_file="$(mktemp "''${TMPDIR:-/tmp}/nixfied-runtime-status.XXXXXX")" || return 1
      if ! ${kernelPackage}/bin/nixfied-kernel registry runtime-status \
        "$service_index_file" \
        "$slot_index_file" \
        "$export_file" >/dev/null; then
        rm -f "$export_file"
        return 1
      fi
      if ! . "$export_file"; then
        rm -f "$export_file"
        return 1
      fi
      rm -f "$export_file"
    }
  '';

  emitEvent = pkgs.writeShellScript "runtime-events-emit-event" ''
    ${sharedPrelude}

    EVENT_TYPE=""
    EVENT_STATE=""
    EVENT_SERVICE=""
    EVENT_RUN_ID=""
    EVENT_COMMAND=""
    EVENT_SLOT=""
    EVENT_ENV=""
    EVENT_PROFILE=""
    EVENT_PID=""
    EVENT_PGID=""
    EVENT_PLAN_ID=""
    EVENT_UNIT_ID=""
    EVENT_ATTEMPT=""
    EVENT_OWNER_SCOPE=""
    EVENT_REUSE_POLICY=""
    EVENT_DISCOVERY_SCOPE=""
    EVENT_EPHEMERAL_ROOT=""
    EVENT_WAIT_REASON=""
    EVENT_LOG_PATH=""
    EVENT_LAST_ERROR=""
    EVENT_READINESS_HEALTH=""
    EVENT_READINESS_READY=""

    while [ "$#" -gt 0 ]; do
      case "$1" in
        --event-type) EVENT_TYPE="$2"; shift 2 ;;
        --state) EVENT_STATE="$2"; shift 2 ;;
        --service) EVENT_SERVICE="$2"; shift 2 ;;
        --run-id) EVENT_RUN_ID="$2"; shift 2 ;;
        --command) EVENT_COMMAND="$2"; shift 2 ;;
        --slot) EVENT_SLOT="$2"; shift 2 ;;
        --env) EVENT_ENV="$2"; shift 2 ;;
        --profile) EVENT_PROFILE="$2"; shift 2 ;;
        --pid) EVENT_PID="$2"; shift 2 ;;
        --pgid) EVENT_PGID="$2"; shift 2 ;;
        --plan-id) EVENT_PLAN_ID="$2"; shift 2 ;;
        --unit-id) EVENT_UNIT_ID="$2"; shift 2 ;;
        --attempt) EVENT_ATTEMPT="$2"; shift 2 ;;
        --owner-scope) EVENT_OWNER_SCOPE="$2"; shift 2 ;;
        --reuse-policy) EVENT_REUSE_POLICY="$2"; shift 2 ;;
        --discovery-scope) EVENT_DISCOVERY_SCOPE="$2"; shift 2 ;;
        --ephemeral-root) EVENT_EPHEMERAL_ROOT="$2"; shift 2 ;;
        --wait-reason) EVENT_WAIT_REASON="$2"; shift 2 ;;
        --log-path) EVENT_LOG_PATH="$2"; shift 2 ;;
        --last-error) EVENT_LAST_ERROR="$2"; shift 2 ;;
        --readiness-health) EVENT_READINESS_HEALTH="$2"; shift 2 ;;
        --readiness-ready) EVENT_READINESS_READY="$2"; shift 2 ;;
        *)
          log_error "unknown argument: $1"
          exit 1
          ;;
      esac
    done

    if [ -z "$EVENT_TYPE" ]; then
      echo "Usage: runtime-events-emit-event --event-type <type> [--service <name>] [--state <state>] [--run-id <id>] [--slot <slot>] [--env <env>] [--wait-reason <reason>] [--log-path <path>]" >&2
      exit 1
    fi

    if [ -z "$EVENT_PLAN_ID" ]; then
      EVENT_PLAN_ID="''${NIXFIED_PLAN_ID:-}"
    fi

    if [ -z "$EVENT_UNIT_ID" ]; then
      EVENT_UNIT_ID="''${NIXFIED_UNIT_ID:-}"
    fi

    if [ -z "$EVENT_ATTEMPT" ]; then
      EVENT_ATTEMPT="''${NIXFIED_UNIT_ATTEMPT:-}"
    fi
    if [ -z "$EVENT_ATTEMPT" ]; then
      EVENT_ATTEMPT="1"
    fi
    case "$EVENT_ATTEMPT" in
      *[!0-9]*|"")
        log_error "--attempt must be a positive integer (got '$EVENT_ATTEMPT')"
        exit 1
        ;;
      0)
        log_error "--attempt must be >= 1 (got '$EVENT_ATTEMPT')"
        exit 1
        ;;
      *)
        ;;
    esac

    if [ -z "$EVENT_RUN_ID" ]; then
      EVENT_RUN_ID="$(${id.resolveId} "''${RUN_ID:-}" "$EVENT_PLAN_ID")"
      export RUN_ID="$EVENT_RUN_ID"
    fi

    if [ -z "$EVENT_COMMAND" ]; then
      EVENT_COMMAND="''${COMMAND_NAME:-unknown}"
    fi

    if [ -z "$EVENT_SLOT" ]; then
      EVENT_SLOT="''${SLOT:-''${!SLOT_VAR:-}}"
    fi

    if [ -z "$EVENT_ENV" ]; then
      EVENT_ENV="''${ENV:-''${!ENV_VAR:-}}"
    fi

    if [ -z "$EVENT_PID" ]; then
      EVENT_PID="$$"
    fi

    if [ -z "$EVENT_PGID" ]; then
      EVENT_PGID="$(${pkgs.procps}/bin/ps -o pgid= -p "$EVENT_PID" 2>/dev/null | tr -d ' ' || true)"
    fi

    if ! load_runtime_event_policy_exports \
      "$EVENT_REUSE_POLICY" \
      "$EVENT_OWNER_SCOPE" \
      "$EVENT_DISCOVERY_SCOPE"; then
      log_error "failed to resolve runtime event service policy event_type=$EVENT_TYPE"
      exit 1
    fi
    EVENT_OWNER_SCOPE="$OWNER_SCOPE"
    EVENT_DISCOVERY_SCOPE="$DISCOVERY_SCOPE"
    EVENT_REUSE_POLICY="$REUSE_POLICY"

    if [ -z "$EVENT_EPHEMERAL_ROOT" ]; then
      EVENT_EPHEMERAL_ROOT="''${!EPHEMERAL_ROOT_VAR:-}"
    fi

    if [ -z "$EVENT_STATE" ]; then
      if ! EVENT_STATE="$(${kernelPackage}/bin/nixfied-kernel event-state derive "$EVENT_TYPE")"; then
        log_error "failed to derive runtime event state event_type=$EVENT_TYPE"
        exit 1
      fi
    fi

    EVENT_READINESS_HEALTH_NORM="$(normalize_bool "$EVENT_READINESS_HEALTH")"
    EVENT_READINESS_READY_NORM="$(normalize_bool "$EVENT_READINESS_READY")"
    EVENT_KIND="slotLifecycle"
    if [ -n "$EVENT_SERVICE" ]; then
      EVENT_KIND="serviceLifecycle"
    fi

    if ! DETAIL_JSON="$(
      ${kernelPackage}/bin/nixfied-kernel event-detail render \
        "$EVENT_KIND" \
        --event-type "$EVENT_TYPE" \
        --command-name "$EVENT_COMMAND" \
        --project-id "$PROJECT_ID" \
        --service "$EVENT_SERVICE" \
        --slot "$EVENT_SLOT" \
        --env "$EVENT_ENV" \
        --profile "$EVENT_PROFILE" \
        --pid "$EVENT_PID" \
        --pgid "$EVENT_PGID" \
        --plan-id "$EVENT_PLAN_ID" \
        --unit-id "$EVENT_UNIT_ID" \
        --attempt "$EVENT_ATTEMPT" \
        --owner-scope "$EVENT_OWNER_SCOPE" \
        --reuse-policy "$EVENT_REUSE_POLICY" \
        --discovery-scope "$EVENT_DISCOVERY_SCOPE" \
        --ephemeral-root "$EVENT_EPHEMERAL_ROOT" \
        --readiness-health "$EVENT_READINESS_HEALTH_NORM" \
        --readiness-ready "$EVENT_READINESS_READY_NORM" \
        --last-error "$EVENT_LAST_ERROR" \
        --wait-reason "$EVENT_WAIT_REASON" \
        --log-path "$EVENT_LOG_PATH"
    )"; then
      log_error "failed to render runtime event detail event_type=$EVENT_TYPE kind=$EVENT_KIND"
      exit 1
    fi

    if ! registry_append_event "$REGISTRY_ROOT" "$EVENT_RUN_ID" "''${NIXFIED_ATTEMPT_ID:-}" "" "" "$EVENT_STATE" "$DETAIL_JSON"; then
      log_error "failed to record runtime event event_type=$EVENT_TYPE run_id=$EVENT_RUN_ID"
      exit 1
    fi

    if [ -n "$EVENT_SERVICE" ] && [ -n "''${REGISTRY_APPEND_LAST_SEQ:-}" ] && [ -n "''${REGISTRY_APPEND_LAST_EVENT_JSON:-}" ]; then
      append_index_line_locked \
        "$(service_events_index_file_for "$EVENT_SERVICE" "$EVENT_SLOT" "$EVENT_ENV")" \
        "''${REGISTRY_APPEND_LAST_SEQ}	''${REGISTRY_APPEND_LAST_EVENT_JSON}" || {
        log_error "failed to update service event index service=$EVENT_SERVICE"
        exit 1
      }
    fi

    if [ "$EVENT_KIND" = "slotLifecycle" ] && [ -n "$EVENT_SLOT" ] && [ -n "$EVENT_ENV" ] \
      && [ -n "''${REGISTRY_APPEND_LAST_SEQ:-}" ] && [ -n "''${REGISTRY_APPEND_LAST_EVENT_JSON:-}" ]; then
      append_index_line_locked \
        "$(slot_events_index_file_for "$EVENT_SLOT" "$EVENT_ENV")" \
        "''${REGISTRY_APPEND_LAST_SEQ}	''${REGISTRY_APPEND_LAST_EVENT_JSON}" || {
        log_error "failed to update slot event index slot=$EVENT_SLOT env=$EVENT_ENV"
        exit 1
      }
    fi

    log_ok "runtime event recorded event_type=$EVENT_TYPE run_id=$EVENT_RUN_ID service=''${EVENT_SERVICE:-none} state=$EVENT_STATE"
  '';

  serviceEvents = pkgs.writeShellScript "service-events" ''
    ${sharedPrelude}

    SERVICE=""
    SLOT_FILTER=""
    ENV_FILTER=""
    LIMIT="200"

    while [ "$#" -gt 0 ]; do
      case "$1" in
        --service) SERVICE="$2"; shift 2 ;;
        --slot) SLOT_FILTER="$2"; shift 2 ;;
        --env) ENV_FILTER="$2"; shift 2 ;;
        --limit) LIMIT="$2"; shift 2 ;;
        *)
          log_error "unknown argument: $1"
          exit 1
          ;;
      esac
    done

    if [ -z "$SERVICE" ]; then
      echo "Usage: service-events --service <name> [--slot <slot>] [--env <env>] [--limit <n>]" >&2
      exit 1
    fi

    case "$LIMIT" in
      *[!0-9]*|"")
        log_error "--limit must be a positive integer (got '$LIMIT')"
        exit 1
        ;;
      *)
        ;;
    esac

    collect_service_event_files() {
      local search_root

      search_root="$(service_events_root_for "$SERVICE")"
      if [ -n "$SLOT_FILTER" ]; then
        search_root="$search_root/$(runtime_index_segment "$SLOT_FILTER")"
      fi
      if [ -n "$ENV_FILTER" ]; then
        search_root="$search_root/$(runtime_index_segment "$ENV_FILTER")"
      fi

      if [ ! -d "$search_root" ]; then
        return 0
      fi

      ${pkgs.findutils}/bin/find "$search_root" -type f -name 'events.tsv' | ${pkgs.coreutils}/bin/sort
    }

    OUT="$(
      while IFS= read -r events_file; do
        [ -f "$events_file" ] || continue
        cat "$events_file"
      done < <(collect_service_event_files) | ${pkgs.coreutils}/bin/sort -t $'\t' -k1,1n | ${pkgs.coreutils}/bin/tail -n "$LIMIT" | ${pkgs.coreutils}/bin/cut -f2-
    )"

    if [ -z "$OUT" ]; then
      log_ok "no events found service=$SERVICE project_id=$PROJECT_ID"
      exit 0
    fi

    echo "$OUT"
  '';

  serviceLogs = pkgs.writeShellScript "service-logs" ''
    ${sharedPrelude}

    SERVICE=""
    SLOT_FILTER=""
    ENV_FILTER=""
    FOLLOW=false
    LINES="200"

    while [ "$#" -gt 0 ]; do
      case "$1" in
        --service) SERVICE="$2"; shift 2 ;;
        --slot) SLOT_FILTER="$2"; shift 2 ;;
        --env) ENV_FILTER="$2"; shift 2 ;;
        --lines) LINES="$2"; shift 2 ;;
        --follow|-f) FOLLOW=true; shift ;;
        *)
          log_error "unknown argument: $1"
          exit 1
          ;;
      esac
    done

    if [ -z "$SERVICE" ]; then
      echo "Usage: service-logs --service <name> [--slot <slot>] [--env <env>] [--lines <n>] [--follow]" >&2
      exit 1
    fi

    case "$LINES" in
      *[!0-9]*|"")
        log_error "--lines must be a positive integer (got '$LINES')"
        exit 1
        ;;
      *)
        ;;
    esac

    if [ -z "$SLOT_FILTER" ]; then
      SLOT_FILTER="''${SLOT:-''${!SLOT_VAR:-}}"
    fi
    if [ -z "$ENV_FILTER" ]; then
      ENV_FILTER="''${ENV:-''${!ENV_VAR:-}}"
    fi

    if ! load_runtime_status_exports "$SERVICE" "$SLOT_FILTER" "$ENV_FILTER"; then
      log_error "failed to resolve runtime status service=$SERVICE"
      exit 1
    fi
    if [ "''${REGISTRY_FOUND:-0}" != "1" ]; then
      log_error "no registry events found; cannot resolve log path service=$SERVICE"
      exit 1
    fi
    LOG_PATH="''${LOG_PATH:-}"

    if [ -z "$LOG_PATH" ]; then
      log_error "no log path recorded for service=$SERVICE slot=''${SLOT_FILTER:-any} env=''${ENV_FILTER:-any}"
      log_hint "start the service once so it emits lifecycle events with logPath."
      exit 1
    fi

    case "$LOG_PATH" in
      /*) ;;
      *)
        log_error "log path must be absolute path=$LOG_PATH service=$SERVICE"
        exit 1
        ;;
    esac
    if echo "$LOG_PATH" | ${pkgs.gnugrep}/bin/grep -Eq '(^|/)[.]{1,2}(/|$)'; then
      log_error "log path contains unsafe traversal segments path=$LOG_PATH service=$SERVICE"
      exit 1
    fi

    LOG_PATH_CANON="$(${pkgs.coreutils}/bin/realpath "$LOG_PATH" 2>/dev/null || true)"
    if [ -z "$LOG_PATH_CANON" ]; then
      log_error "failed to resolve canonical log path path=$LOG_PATH service=$SERVICE"
      exit 1
    fi

    ALLOWED_ROOTS=()
    add_allowed_root() {
      local root_path="$1"
      local root_canon
      if [ -z "$root_path" ] || [ ! -d "$root_path" ]; then
        return 0
      fi
      root_canon="$(${pkgs.coreutils}/bin/realpath "$root_path" 2>/dev/null || true)"
      if [ -n "$root_canon" ]; then
        ALLOWED_ROOTS+=("$root_canon")
      fi
      return 0
    }

    is_under_root() {
      local candidate="$1"
      local root="$2"
      case "$candidate" in
        "$root"|"$root"/*) return 0 ;;
        *) return 1 ;;
      esac
    }

    add_allowed_root "''${BASE_DIR:-$BASE_DIR_DEFAULT}"
    add_allowed_root "$CI_ARTIFACTS_BASE_DEFAULT"
    add_allowed_root "''${CI_ARTIFACTS_BASE:-}"
    add_allowed_root "''${CI_ARTIFACTS_DIR:-}"

    case "$LOG_PATH_CANON" in
      "$EPHEMERAL_PREFIX"*)
        eph_suffix="''${LOG_PATH_CANON#$EPHEMERAL_PREFIX}"
        eph_id="''${eph_suffix%%/*}"
        if [ -n "$eph_id" ] && [ "$eph_id" != "$eph_suffix" -o "$LOG_PATH_CANON" = "$EPHEMERAL_PREFIX$eph_id" ]; then
          add_allowed_root "$EPHEMERAL_PREFIX$eph_id"
        fi
        ;;
      *)
        ;;
    esac

    ALLOWED=0
    for root in "''${ALLOWED_ROOTS[@]}"; do
      if is_under_root "$LOG_PATH_CANON" "$root"; then
        ALLOWED=1
        break
      fi
    done
    if [ "$ALLOWED" -ne 1 ]; then
      log_error "log path is outside allowed roots path=$LOG_PATH_CANON service=$SERVICE"
      log_hint "allowed roots are BASE_DIR/CI_ARTIFACTS and project ephemeral prefixes only."
      exit 1
    fi

    if [ ! -f "$LOG_PATH_CANON" ]; then
      log_error "log file not found path=$LOG_PATH_CANON service=$SERVICE"
      exit 1
    fi

    if [ "$FOLLOW" = "true" ]; then
      exec ${pkgs.coreutils}/bin/tail -n "$LINES" -f "$LOG_PATH_CANON"
    fi
    exec ${pkgs.coreutils}/bin/tail -n "$LINES" "$LOG_PATH_CANON"
  '';

  serviceStatus = pkgs.writeShellScript "service-status" ''
    ${sharedPrelude}

    SERVICE=""
    SLOT_FILTER=""
    ENV_FILTER=""

    emit_var() {
      local key="$1"
      local value="$2"
      printf '%s=%q\n' "$key" "$value"
    }

    while [ "$#" -gt 0 ]; do
      case "$1" in
        --service) SERVICE="$2"; shift 2 ;;
        --slot) SLOT_FILTER="$2"; shift 2 ;;
        --env) ENV_FILTER="$2"; shift 2 ;;
        *)
          log_error "unknown argument: $1"
          exit 1
          ;;
      esac
    done

    if [ -z "$SERVICE" ]; then
      echo "Usage: service-status --service <name> [--slot <slot>] [--env <env>]" >&2
      exit 1
    fi

    if [ -z "$SLOT_FILTER" ]; then
      SLOT_FILTER="''${SLOT:-''${!SLOT_VAR:-}}"
    fi
    if [ -z "$ENV_FILTER" ]; then
      ENV_FILTER="''${ENV:-''${!ENV_VAR:-}}"
    fi

    if ! load_runtime_event_policy_exports "" "" ""; then
      log_error "failed to resolve runtime event service policy service=$SERVICE"
      exit 1
    fi
    RESOLVED_DISCOVERY_SCOPE="$DISCOVERY_SCOPE"

    if [ "$RESOLVED_DISCOVERY_SCOPE" = "local" ]; then
      emit_var "REGISTRY_FOUND" "0"
      emit_var "REGISTRY_RUNNING" "false"
      emit_var "REGISTRY_SCOPE" "local"
      emit_var "REGISTRY_STATE" "local_only"
      emit_var "OWNER_RUN_ID" ""
      emit_var "OWNER_SCOPE" ""
      emit_var "EPHEMERAL_ROOT" ""
      emit_var "WAIT_REASON" ""
      emit_var "LOG_PATH" ""
      emit_var "SLOT_OWNER" ""
      exit 0
    fi

    if ! load_runtime_status_exports "$SERVICE" "$SLOT_FILTER" "$ENV_FILTER"; then
      log_error "failed to resolve runtime status service=$SERVICE"
      exit 1
    fi

    emit_var "REGISTRY_FOUND" "''${REGISTRY_FOUND:-0}"
    emit_var "REGISTRY_RUNNING" "''${REGISTRY_RUNNING:-false}"
    emit_var "REGISTRY_SCOPE" "global"
    emit_var "REGISTRY_STATE" "''${REGISTRY_STATE:-unknown}"
    emit_var "OWNER_RUN_ID" "''${OWNER_RUN_ID:-}"
    emit_var "OWNER_SCOPE" "''${OWNER_SCOPE:-}"
    emit_var "EPHEMERAL_ROOT" "''${EPHEMERAL_ROOT:-}"
    emit_var "WAIT_REASON" "''${WAIT_REASON:-}"
    emit_var "LOG_PATH" "''${LOG_PATH:-}"
    emit_var "SLOT_OWNER" "''${SLOT_OWNER:-}"
  '';
in
{
  inherit
    registryRoot
    emitEvent
    serviceEvents
    serviceLogs
    serviceStatus
    ;
}
