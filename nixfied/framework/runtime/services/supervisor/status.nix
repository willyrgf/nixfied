# Supervisor status and log viewing
{
  pkgs,
  slots,
  config,
  runtime,
  loggingPrelude,
}:

let
  pc = pkgs.process-compose;
  kernelPackage = import ../../kernel { inherit pkgs; };

  status = runtime.mkSupervisorScript {
    name = "supervisor-status";
    body = ''
      if ! ${isRunning} >/dev/null 2>&1; then
        echo "service=supervisor slot=$SLOT env=$ENV running=false"
        exit 1
      fi

      exec ${pc}/bin/process-compose process list -o wide
    '';
  };

  isRunning = runtime.mkSupervisorScript {
    name = "supervisor-is-running";
    useLoggingPrelude = false;
    body = ''
      if supervisor_process_api_ready; then
        echo "running (socket $PC_SOCKET_PATH)"
        exit 0
      fi

      echo "stopped"
      exit 1
    '';
  };

  logs = runtime.mkSupervisorScript {
    name = "supervisor-logs";
    includeLogDir = true;
    useLoggingPrelude = false;
    body = ''
      SERVICE="''${1:-}"
      LINES="''${2:-50}"

      if [ -n "$SERVICE" ]; then
        LOG_FILE="$LOG_DIR/$SERVICE.log"
        if [ -f "$LOG_FILE" ]; then
          tail -n "$LINES" -f "$LOG_FILE"
        else
          echo "No log file for service: $SERVICE" >&2
          echo "Looking in $LOG_DIR..." >&2
          ls "$LOG_DIR"/*.log 2>/dev/null || echo "No logs found"
          exit 1
        fi
      else
        LOG_FILE="$LOG_DIR/supervisor.log"
        if [ -f "$LOG_FILE" ]; then
          tail -n "$LINES" -f "$LOG_FILE"
        else
          echo "No supervisor log found at $LOG_FILE" >&2
          exit 1
        fi
      fi
    '';
  };

  health = runtime.mkSupervisorScript {
    name = "supervisor-health";
    body = ''
      if ! supervisor_process_api_ready; then
        log_error "supervisor unhealthy reason=daemon_not_running slot=$SLOT env=$ENV"
        exit 1
      fi

      if ! supervisor_fetch_process_json; then
        log_error "supervisor unhealthy reason=process_query_failed socket=$PC_SOCKET_PATH"
        exit 1
      fi

      TOTAL=$(
        ${kernelPackage}/bin/nixfied-kernel json-length - <<<"$SUPERVISOR_PROCESS_JSON"
      )
      if [ "$TOTAL" -eq 0 ]; then
        log_ok "supervisor healthy services=0 slot=$SLOT env=$ENV"
        exit 0
      fi

      UNHEALTHY=$(
        total_index=$((TOTAL - 1))
        idx=0
        parts=""
        while [ "$idx" -le "$total_index" ]; do
          status="$(${kernelPackage}/bin/nixfied-kernel query-json - ".''${idx}.status" --raw <<<"$SUPERVISOR_PROCESS_JSON")"
          running="$(${kernelPackage}/bin/nixfied-kernel query-json - ".''${idx}.is_running" --raw <<<"$SUPERVISOR_PROCESS_JSON")"
          ready="$(${kernelPackage}/bin/nixfied-kernel query-json - ".''${idx}.is_ready" --raw <<<"$SUPERVISOR_PROCESS_JSON")"
          name="$(${kernelPackage}/bin/nixfied-kernel query-json - ".''${idx}.name" --raw <<<"$SUPERVISOR_PROCESS_JSON")"
          if [ "$status" != "Running" ] || [ "$running" != "true" ]; then
            entry="''${name}:status=$status,running=$running,ready=$ready"
            if [ -n "$parts" ]; then
              parts="$parts; $entry"
            else
              parts="$entry"
            fi
          fi
          idx=$((idx + 1))
        done
        printf '%s' "$parts"
      )

      if [ -n "$UNHEALTHY" ]; then
        log_error "supervisor unhealthy slot=$SLOT env=$ENV services=$UNHEALTHY"
        exit 1
      fi

      log_ok "supervisor healthy services=$TOTAL slot=$SLOT env=$ENV"
    '';
  };

in
{
  inherit
    status
    isRunning
    health
    logs
    ;
}
