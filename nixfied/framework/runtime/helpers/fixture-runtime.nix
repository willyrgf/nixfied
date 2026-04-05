{ }:

{
  fixtureRuntime = ''
    _service_op_available() {
      local service="$1"
      local op="$2"

      if [ -z "$service" ] || [ -z "$op" ] || [ -z "''${NIXFIED_RUNTIME_BIN:-}" ]; then
        return 1
      fi

      "$NIXFIED_RUNTIME_BIN" has-service-op "$service" "$op" >/dev/null 2>&1
    }

    # fixture_start_service SERVICE [profile] [timeout] [interval] [logfile] [keep_running]
    # - start service lifecycle from the runtime service ABI and register cleanup unless keep_running=1.
    #
    # Lifecycle policy invariants:
    # - start ops may return before the service is externally ready.
    # - READY/HEALTH ops must be safe to poll and deterministic when not ready.
    # - fixtures must poll READY/HEALTH with timeout loops after start (never one-shot checks).
    # - failure diagnostics must not introduce secondary errors (for example tailing missing logs).
    fixture_start_service() {
      local service="$1"
      local profile="''${2:-default}"
      local timeout="''${3:-60}"
      local interval="''${4:-1}"
      local logfile="''${5:-}"
      local keep_running_raw="''${6:-0}"
      local keep_running="0"

      if [ -z "$service" ]; then
        echo "usage: fixture_start_service <service> [profile] [timeout] [interval] [logfile] [keep_running]" >&2
        return 1
      fi

      case "$keep_running_raw" in
        1|true|TRUE|yes|YES)
          keep_running="1"
          ;;
        0|false|FALSE|no|NO|"")
          keep_running="0"
          ;;
        *)
          log_error "fixture_start_service keep_running must be 0/1/true/false (got '$keep_running_raw')"
          return 1
          ;;
      esac
      require_positive_int "timeout" "$timeout" || return 1
      require_positive_number "interval" "$interval" || return 1

      local start_op=""
      local stop_op="stop"
      local health_op="health"
      local ready_op="ready"
      local wait_op=""
      local -a start_cmd=()

      if _service_op_available "$service" start; then
        start_op="start"
      else
        log_error "fixture_start_service could not resolve canonical start op service=$service profile=$profile"
        return 1
      fi

      start_cmd=("$NIXFIED_RUNTIME_BIN" run-service "$service" "$start_op")

      local pid=""
      if [ -n "$logfile" ]; then
        if ! NIXFIED_START_SERVICE_MANAGED_CLEANUP=1 start_service_into pid "$service" --log "$logfile" -- "''${start_cmd[@]}"; then
          log_error "fixture service start failed service=$service op=$start_op"
          return 1
        fi
      else
        if ! NIXFIED_START_SERVICE_MANAGED_CLEANUP=1 start_service_into pid "$service" -- "''${start_cmd[@]}"; then
          log_error "fixture service start failed service=$service op=$start_op"
          return 1
        fi
      fi
      if [ -z "$pid" ]; then
        log_error "fixture service start returned empty pid service=$service op=$start_op"
        return 1
      fi

      # Clean wrapper process and module-native process state unless persistence is requested.
      if [ "$keep_running" = "1" ]; then
        log_info "fixture service keep_running enabled service=$service pid=$pid"
      else
        with_cleanup stop_service "$pid" "$service"
        if _service_op_available "$service" "$stop_op"; then
          with_cleanup svc "$service" "$stop_op"
        fi
      fi

      if _service_op_available "$service" "$ready_op"; then
        wait_op="$ready_op"
      elif _service_op_available "$service" "$health_op"; then
        wait_op="$health_op"
      fi

      if [ -n "$wait_op" ]; then
        local start_ts
        start_ts=$(date +%s)

        while true; do
          if svc "$service" "$wait_op" >/dev/null 2>&1; then
            break
          fi

          # If the wrapper process died, fail fast unless STATUS indicates service is up.
          if ! kill -0 "$pid" 2>/dev/null; then
            local status_ok=0
            if _service_op_available "$service" status; then
              if svc "$service" status >/dev/null 2>&1; then
                status_ok=1
              fi
            fi

            if [ "$status_ok" -ne 1 ]; then
              log_error "fixture service start exited early service=$service pid=$pid op=$start_op"
              if [ -n "$logfile" ]; then
                print_log_tail "$logfile" 200
              fi
              if _service_op_available "$service" "$stop_op"; then
                svc "$service" "$stop_op" >/dev/null 2>&1 || true
              fi
              stop_service "$pid" "$service" >/dev/null 2>&1 || true
              return 1
            fi
          fi

          if [ $(( $(date +%s) - start_ts )) -ge "$timeout" ]; then
            log_error "fixture readiness check failed service=$service op=$wait_op timeout=''${timeout}s"
            if [ -n "$logfile" ]; then
              print_log_tail "$logfile" 200
            fi
            if _service_op_available "$service" "$stop_op"; then
              svc "$service" "$stop_op" >/dev/null 2>&1 || true
            fi
            stop_service "$pid" "$service" >/dev/null 2>&1 || true
            return 1
          fi

          sleep "$interval"
        done
      fi

      log_ok "fixture service ready service=$service profile=$profile"
      return 0
    }
  '';
}
