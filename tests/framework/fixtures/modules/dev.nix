{ project, ... }:

{
  commands = {
    dev = {
      description = "Module integration tests";
      env = {
        "${project.envVar}" = "dev";
        "${project.slotVar}" = "1";
      };
      useDeps = false;
      script = ''
        set -euo pipefail

        require_env "${project.envVar}"
        if skip_if_missing "THIS_SHOULD_BE_MISSING" "missing on purpose"; then
          echo "skip_if_missing should have returned non-zero" >&2
          exit 1
        fi

        if [ -z "''${SLOT_INFO:-}" ] || [ ! -x "$SLOT_INFO" ]; then
          echo "SLOT_INFO not executable: ''${SLOT_INFO:-<unset>}" >&2
          exit 1
        fi
        SLOT_INFO_OUT=".slot-info.out"
        "$SLOT_INFO" > "$SLOT_INFO_OUT"
        if [ "''${NIXFIED_TEST_DEBUG:-}" = "1" ]; then
          echo "SLOT_INFO=$SLOT_INFO" >&2
          echo "SLOT_INFO output:" >&2
          cat "$SLOT_INFO_OUT" >&2
        fi
        eval "$(cat "$SLOT_INFO_OUT")"

        if [ "$ENV" != "dev" ]; then
          echo "ENV mismatch: $ENV" >&2
          exit 1
        fi

        if [ "$SLOT" != "1" ]; then
          echo "SLOT mismatch: $SLOT" >&2
          exit 1
        fi

        if [ "$BACKEND_PORT" -ne 3011 ]; then
          echo "BACKEND_PORT mismatch: $BACKEND_PORT" >&2
          exit 1
        fi

        export PGPORT="$POSTGRES_PORT"
        export PGDATA="$BASE_DIR/postgres-$SLOT-$ENV"
        run_hook POSTGRES_INIT

        if [ ! -f "$PGDATA/postgresql.conf" ]; then
          echo "postgresql.conf missing" >&2
          exit 1
        fi

        NGINX_DIR="$BASE_DIR/nginx-$SLOT-$ENV"
        run_hook NGINX_INIT
        run_hook NGINX_SITE_PROXY example.localhost 127.0.0.1 "$BACKEND_PORT"

        if [ ! -f "$NGINX_DIR/conf/nginx.conf" ]; then
          echo "nginx.conf missing" >&2
          exit 1
        fi

        if [ ! -f "$NGINX_DIR/conf/sites-enabled/example.localhost.conf" ]; then
          echo "nginx site missing" >&2
          exit 1
        fi

        mkdir -p "$BASE_DIR/logs-$SLOT-$ENV"
        LOGFILE="$BASE_DIR/logs-$SLOT-$ENV/helpers.log"
        log_capture "$LOGFILE" -- echo "helpers ok"
        if ! grep -q "helpers ok" "$LOGFILE"; then
          echo "log_capture failed" >&2
          exit 1
        fi

        PID=$(start_service sleeper -- sleep 5)
        if ! kill -0 "$PID" 2>/dev/null; then
          echo "start_service failed" >&2
          exit 1
        fi
        stop_service "$PID" "sleeper"

        echo "modules fixture ok"
      '';
    };
  };
}
