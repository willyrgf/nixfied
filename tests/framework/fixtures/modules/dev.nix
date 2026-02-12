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

        pick_port() {
          local port
          local i
          for i in $(seq 1 40); do
            port=$(( (RANDOM % 20000) + 20000 ))
            if ! nc -z 127.0.0.1 "$port" >/dev/null 2>&1; then
              echo "$port"
              return 0
            fi
          done
          return 1
        }

        export PGPORT="$(pick_port)"
        if [ -z "''${PGPORT:-}" ]; then
          echo "failed to pick postgres port" >&2
          exit 1
        fi
        export PGDATA="$BASE_DIR/postgres-$SLOT-$ENV"
        run_hook POSTGRES_INIT

        if [ ! -f "$PGDATA/postgresql.conf" ]; then
          echo "postgresql.conf missing" >&2
          exit 1
        fi

        if [ ! -f "$PGDATA/PG_VERSION" ]; then
          echo "PG_VERSION missing" >&2
          exit 1
        fi

        if grep -q '\$PGPORT' "$PGDATA/postgresql.conf"; then
          echo "postgresql.conf still contains unresolved PGPORT placeholder" >&2
          exit 1
        fi

        if ! grep -Eq "^[[:space:]]*port[[:space:]]*=[[:space:]]*$PGPORT([[:space:]]|$)" "$PGDATA/postgresql.conf"; then
          echo "postgresql.conf did not render runtime PGPORT ($PGPORT)" >&2
          exit 1
        fi

        run_hook POSTGRES_INIT
        run_hook POSTGRES_CHECK_CONFIG

        POSTGRES_STARTED=0
        if nc -z 127.0.0.1 "$PGPORT" >/dev/null 2>&1; then
          echo "Skipping POSTGRES_START (port in use)" >&2
        else
          set +e
          run_hook POSTGRES_START
          POSTGRES_RC=$?
          set -e
          if [ "$POSTGRES_RC" -ne 0 ]; then
            echo "Skipping POSTGRES_START checks (failed to start)" >&2
            tail -50 "$PGDATA/postgres.log" >&2 || true
          else
            POSTGRES_STARTED=1
          fi
        fi

        if [ "$POSTGRES_STARTED" -eq 1 ]; then
          for i in $(seq 1 20); do
            if [ -f "$PGDATA/postmaster.pid" ]; then
              break
            fi
            sleep 0.2
          done
          if [ ! -f "$PGDATA/postmaster.pid" ]; then
            echo "postmaster.pid missing after start" >&2
            exit 1
          fi

          export PGDATABASE="nixfied_test"
          run_hook POSTGRES_SETUP_DB

          run_hook POSTGRES_STOP
          for i in $(seq 1 20); do
            if [ ! -f "$PGDATA/postmaster.pid" ]; then
              break
            fi
            sleep 0.2
          done
          if [ -f "$PGDATA/postmaster.pid" ]; then
            echo "postmaster.pid still present after stop" >&2
            exit 1
          fi
        fi

        NGINX_DIR="$BASE_DIR/nginx-$SLOT-$ENV"
        run_hook NGINX_INIT
        run_hook NGINX_INIT
        run_hook NGINX_SITE_PROXY example.localhost 127.0.0.1 "$BACKEND_PORT"
        run_hook NGINX_SITE_PROXY example.localhost 127.0.0.1 "$BACKEND_PORT"
        STATIC_ROOT="$NGINX_DIR/static"
        mkdir -p "$STATIC_ROOT"
        echo "static ok" > "$STATIC_ROOT/index.html"
        run_hook NGINX_SITE_STATIC static.localhost "$STATIC_ROOT"

        if [ ! -f "$NGINX_DIR/conf/nginx.conf" ]; then
          echo "nginx.conf missing" >&2
          exit 1
        fi

        if [ ! -f "$NGINX_DIR/conf/sites-enabled/example.localhost.conf" ]; then
          echo "nginx site missing" >&2
          exit 1
        fi

        if [ ! -f "$NGINX_DIR/conf/sites-enabled/static.localhost.conf" ]; then
          echo "nginx static site missing" >&2
          exit 1
        fi

        # Example: module hooks should fail cleanly when slot/env are missing.
        unset "${project.envVar}" "${project.slotVar}" NIXFIED_ENV
        if run_hook POSTGRES_START >/dev/null 2>&1; then
          echo "POSTGRES_START should fail without slot/env" >&2
          exit 1
        fi
        if run_hook POSTGRES_INIT >/dev/null 2>&1; then
          echo "POSTGRES_INIT should fail without slot/env" >&2
          exit 1
        fi
        if run_hook POSTGRES_SETUP_DB >/dev/null 2>&1; then
          echo "POSTGRES_SETUP_DB should fail without slot/env" >&2
          exit 1
        fi
        export "${project.envVar}"="dev"
        export "${project.slotVar}"="1"
        eval "$("$SLOT_INFO")"

        if run_hook NGINX_SITE_PROXY >/dev/null 2>&1; then
          echo "NGINX_SITE_PROXY should fail without args" >&2
          exit 1
        fi
        if run_hook NGINX_SITE_STATIC >/dev/null 2>&1; then
          echo "NGINX_SITE_STATIC should fail without args" >&2
          exit 1
        fi

        if nc -z 127.0.0.1 "$HTTP_PORT" >/dev/null 2>&1 || nc -z 127.0.0.1 "$HTTPS_PORT" >/dev/null 2>&1; then
          echo "Skipping NGINX_START (port in use)" >&2
        else
          NGINX_PID=$(start_service nginx -- "$NGINX_START")
          for i in $(seq 1 20); do
            if [ -f "$NGINX_DIR/run/nginx.pid" ]; then
              break
            fi
            sleep 0.2
          done
          if [ ! -f "$NGINX_DIR/run/nginx.pid" ]; then
            echo "nginx pid missing after start" >&2
            exit 1
          fi

          run_hook NGINX_STOP
          for i in $(seq 1 20); do
            if [ ! -f "$NGINX_DIR/run/nginx.pid" ]; then
              break
            fi
            sleep 0.2
          done
          if [ -f "$NGINX_DIR/run/nginx.pid" ]; then
            echo "nginx pid still present after stop" >&2
            exit 1
          fi
          if kill -0 "$NGINX_PID" 2>/dev/null; then
            stop_service "$NGINX_PID" "nginx"
          fi
        fi

        MINIO_DIR="$BASE_DIR/minio-$SLOT-$ENV"
        run_hook MINIO_INIT
        run_hook MINIO_INIT
        if [ ! -d "$MINIO_DIR/data" ]; then
          echo "minio data dir missing" >&2
          exit 1
        fi
        run_hook MINIO_CHECK_CONFIG

        if nc -z 127.0.0.1 "$MINIOAPI_PORT" >/dev/null 2>&1 || nc -z 127.0.0.1 "$MINIOCONSOLE_PORT" >/dev/null 2>&1; then
          echo "Skipping MINIO_START (port in use)" >&2
        else
          MINIO_PID=$(start_service minio -- "$MINIO_START")
          MINIO_READY=0
          for i in $(seq 1 50); do
            if run_hook MINIO_HEALTH >/dev/null 2>&1; then
              MINIO_READY=1
              break
            fi
            sleep 0.2
          done
          if [ "$MINIO_READY" -ne 1 ]; then
            echo "minio health check failed after start" >&2
            exit 1
          fi

          run_hook MINIO_BUCKET_LIST >/dev/null
          run_hook MINIO_STOP

          if kill -0 "$MINIO_PID" 2>/dev/null; then
            stop_service "$MINIO_PID" "minio"
          fi
        fi

        RETH_DIR="$BASE_DIR/reth-$SLOT-$ENV"
        run_hook RETH_INIT
        run_hook RETH_INIT
        if [ ! -d "$RETH_DIR/data" ]; then
          echo "reth data dir missing" >&2
          exit 1
        fi
        run_hook RETH_CHECK_CONFIG

        if nc -z 127.0.0.1 "$RETHHTTP_PORT" >/dev/null 2>&1; then
          echo "Skipping RETH_START (port in use)" >&2
        else
          RETH_PID=$(start_service reth -- "$RETH_START")
          RETH_READY=0
          for i in $(seq 1 50); do
            if run_hook RETH_HEALTH >/dev/null 2>&1; then
              RETH_READY=1
              break
            fi
            sleep 0.2
          done
          if [ "$RETH_READY" -ne 1 ]; then
            echo "reth health check failed after start" >&2
            exit 1
          fi

          run_hook RETH_STOP
          if kill -0 "$RETH_PID" 2>/dev/null; then
            stop_service "$RETH_PID" "reth"
          fi
        fi

        HELIOS_DIR="$BASE_DIR/helios-$SLOT-$ENV"
        run_hook HELIOS_INIT
        run_hook HELIOS_INIT
        if [ ! -d "$HELIOS_DIR/data" ]; then
          echo "helios data dir missing" >&2
          exit 1
        fi
        run_hook HELIOS_CHECK_CONFIG

        if nc -z 127.0.0.1 "$HELIOSRPC_PORT" >/dev/null 2>&1; then
          echo "Skipping HELIOS_START (port in use)" >&2
        else
          HELIOS_PID=$(start_service helios -- "$HELIOS_START")
          HELIOS_READY=0
          for i in $(seq 1 50); do
            if run_hook HELIOS_HEALTH >/dev/null 2>&1; then
              HELIOS_READY=1
              break
            fi
            sleep 0.2
          done
          if [ "$HELIOS_READY" -ne 1 ]; then
            echo "helios health check failed after start" >&2
            exit 1
          fi

          run_hook HELIOS_STOP
          if kill -0 "$HELIOS_PID" 2>/dev/null; then
            stop_service "$HELIOS_PID" "helios"
          fi
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
