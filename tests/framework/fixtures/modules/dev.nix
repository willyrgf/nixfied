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

        used_ports=""
        assign_port() {
          local var_name="$1"
          local p
          while true; do
            p="$(pick_port)" || return 1
            case " $used_ports " in
              *" $p "*) ;;
              *)
                used_ports="$used_ports $p"
                export "$var_name=$p"
                return 0
                ;;
            esac
          done
        }

        assign_port PGPORT || {
          echo "failed to pick postgres port" >&2
          exit 1
        }
        assign_port MINIOAPI_PORT || {
          echo "failed to pick minio api port" >&2
          exit 1
        }
        assign_port MINIOCONSOLE_PORT || {
          echo "failed to pick minio console port" >&2
          exit 1
        }
        assign_port RETHHTTP_PORT || {
          echo "failed to pick reth http port" >&2
          exit 1
        }
        assign_port RETHWS_PORT || {
          echo "failed to pick reth ws port" >&2
          exit 1
        }
        assign_port RETHAUTH_PORT || {
          echo "failed to pick reth auth port" >&2
          exit 1
        }
        assign_port HELIOSRPC_PORT || {
          echo "failed to pick helios rpc port" >&2
          exit 1
        }

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
          run_hook POSTGRES_HEALTH >/dev/null 2>&1
          POSTGRES_PRE_HEALTH_RC=$?
          set -e
          if [ "$POSTGRES_PRE_HEALTH_RC" -eq 0 ]; then
            echo "POSTGRES_HEALTH should fail before start" >&2
            exit 1
          fi

          set +e
          run_hook POSTGRES_READY >/dev/null 2>&1
          POSTGRES_PRE_READY_RC=$?
          set -e
          if [ "$POSTGRES_PRE_READY_RC" -eq 0 ]; then
            echo "POSTGRES_READY should fail before start" >&2
            exit 1
          fi

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

          run_hook POSTGRES_HEALTH >/dev/null || {
            echo "POSTGRES_HEALTH should pass while running" >&2
            exit 1
          }
          run_hook POSTGRES_READY >/dev/null || {
            echo "POSTGRES_READY should pass while running" >&2
            exit 1
          }

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

          set +e
          run_hook POSTGRES_HEALTH >/dev/null 2>&1
          POSTGRES_POST_HEALTH_RC=$?
          set -e
          if [ "$POSTGRES_POST_HEALTH_RC" -eq 0 ]; then
            echo "POSTGRES_HEALTH should fail after stop" >&2
            exit 1
          fi

          set +e
          run_hook POSTGRES_READY >/dev/null 2>&1
          POSTGRES_POST_READY_RC=$?
          set -e
          if [ "$POSTGRES_POST_READY_RC" -eq 0 ]; then
            echo "POSTGRES_READY should fail after stop" >&2
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
        if run_hook POSTGRES_HEALTH >/dev/null 2>&1; then
          echo "POSTGRES_HEALTH should fail without slot/env" >&2
          exit 1
        fi
        if run_hook POSTGRES_READY >/dev/null 2>&1; then
          echo "POSTGRES_READY should fail without slot/env" >&2
          exit 1
        fi
        if run_hook NGINX_HEALTH >/dev/null 2>&1; then
          echo "NGINX_HEALTH should fail without slot/env" >&2
          exit 1
        fi
        if run_hook NGINX_READY >/dev/null 2>&1; then
          echo "NGINX_READY should fail without slot/env" >&2
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
          set +e
          run_hook NGINX_HEALTH >/dev/null 2>&1
          NGINX_PRE_HEALTH_RC=$?
          set -e
          if [ "$NGINX_PRE_HEALTH_RC" -eq 0 ]; then
            echo "NGINX_HEALTH should fail before start" >&2
            exit 1
          fi

          set +e
          run_hook NGINX_READY >/dev/null 2>&1
          NGINX_PRE_READY_RC=$?
          set -e
          if [ "$NGINX_PRE_READY_RC" -eq 0 ]; then
            echo "NGINX_READY should fail before start" >&2
            exit 1
          fi

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

          NGINX_READY_OK=0
          for i in $(seq 1 40); do
            if run_hook NGINX_READY >/dev/null 2>&1; then
              NGINX_READY_OK=1
              break
            fi
            sleep 0.2
          done
          if [ "$NGINX_READY_OK" -ne 1 ]; then
            echo "NGINX_READY should pass while running" >&2
            exit 1
          fi

          run_hook NGINX_HEALTH >/dev/null || {
            echo "NGINX_HEALTH should pass while running" >&2
            exit 1
          }

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

          set +e
          run_hook NGINX_HEALTH >/dev/null 2>&1
          NGINX_POST_HEALTH_RC=$?
          set -e
          if [ "$NGINX_POST_HEALTH_RC" -eq 0 ]; then
            echo "NGINX_HEALTH should fail after stop" >&2
            exit 1
          fi
          set +e
          run_hook NGINX_READY >/dev/null 2>&1
          NGINX_POST_READY_RC=$?
          set -e
          if [ "$NGINX_POST_READY_RC" -eq 0 ]; then
            echo "NGINX_READY should fail after stop" >&2
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
          set +e
          run_hook MINIO_HEALTH >/dev/null 2>&1
          MINIO_PRE_HEALTH_RC=$?
          set -e
          if [ "$MINIO_PRE_HEALTH_RC" -eq 0 ]; then
            echo "MINIO_HEALTH should fail before start" >&2
            exit 1
          fi

          set +e
          run_hook MINIO_READY >/dev/null 2>&1
          MINIO_PRE_READY_RC=$?
          set -e
          if [ "$MINIO_PRE_READY_RC" -eq 0 ]; then
            echo "MINIO_READY should fail before start" >&2
            exit 1
          fi

          MINIO_PID=$(start_service minio -- "$MINIO_START")
          MINIO_READY_OK=0
          for i in $(seq 1 50); do
            if run_hook MINIO_READY >/dev/null 2>&1; then
              MINIO_READY_OK=1
              break
            fi
            sleep 0.2
          done
          if [ "$MINIO_READY_OK" -ne 1 ]; then
            echo "minio ready check failed after start" >&2
            exit 1
          fi

          run_hook MINIO_HEALTH >/dev/null || {
            echo "MINIO_HEALTH should pass while running" >&2
            exit 1
          }

          run_hook MINIO_BUCKET_LIST >/dev/null
          run_hook MINIO_STOP

          MINIO_DOWN=0
          for i in $(seq 1 40); do
            if run_hook MINIO_HEALTH >/dev/null 2>&1; then
              sleep 0.2
            else
              MINIO_DOWN=1
              break
            fi
          done
          if [ "$MINIO_DOWN" -ne 1 ]; then
            echo "MINIO_HEALTH should fail after stop" >&2
            exit 1
          fi

          set +e
          run_hook MINIO_READY >/dev/null 2>&1
          MINIO_POST_READY_RC=$?
          set -e
          if [ "$MINIO_POST_READY_RC" -eq 0 ]; then
            echo "MINIO_READY should fail after stop" >&2
            exit 1
          fi

          if kill -0 "$MINIO_PID" 2>/dev/null; then
            stop_service "$MINIO_PID" "minio"
          fi
        fi

        RETH_DIR="$BASE_DIR/reth-$SLOT-$ENV"
        RETH_STARTED=0
        RETH_AVAILABLE=0
        RETH_PID=""
        run_hook RETH_INIT
        run_hook RETH_INIT
        if [ ! -d "$RETH_DIR/data" ]; then
          echo "reth data dir missing" >&2
          exit 1
        fi
        run_hook RETH_CHECK_CONFIG

        if nc -z 127.0.0.1 "$RETHHTTP_PORT" >/dev/null 2>&1 || nc -z 127.0.0.1 "$RETHWS_PORT" >/dev/null 2>&1 || nc -z 127.0.0.1 "$RETHAUTH_PORT" >/dev/null 2>&1; then
          echo "Skipping RETH_START (port in use)" >&2
          if run_hook RETH_HEALTH >/dev/null 2>&1; then
            RETH_AVAILABLE=1
          fi
        else
          set +e
          run_hook RETH_HEALTH >/dev/null 2>&1
          RETH_PRE_HEALTH_RC=$?
          set -e
          if [ "$RETH_PRE_HEALTH_RC" -eq 0 ]; then
            echo "RETH_HEALTH should fail before start" >&2
            exit 1
          fi

          set +e
          run_hook RETH_READY >/dev/null 2>&1
          RETH_PRE_READY_RC=$?
          set -e
          if [ "$RETH_PRE_READY_RC" -eq 0 ]; then
            echo "RETH_READY should fail before start" >&2
            exit 1
          fi

          RETH_PID=$(start_service reth -- "$RETH_START")
          RETH_HEALTH_OK=0
          for i in $(seq 1 120); do
            if run_hook RETH_HEALTH >/dev/null 2>&1; then
              RETH_HEALTH_OK=1
              break
            fi
            sleep 0.2
          done
          if [ "$RETH_HEALTH_OK" -ne 1 ]; then
            echo "reth health check failed after start" >&2
            tail -50 "$RETH_DIR/logs/reth.log" >&2 || true
            exit 1
          fi

          run_hook RETH_HEALTH >/dev/null || {
            echo "RETH_HEALTH should pass while running" >&2
            exit 1
          }
          run_hook RETH_READY >/dev/null || {
            echo "RETH_READY should pass while running" >&2
            exit 1
          }
          RETH_STARTED=1
          RETH_AVAILABLE=1
        fi

        if [ "$RETH_AVAILABLE" -ne 1 ]; then
          echo "Skipping HELIOS_START (reth unavailable)" >&2
        else
          HELIOS_DIR="$BASE_DIR/helios-$SLOT-$ENV"
          run_hook HELIOS_INIT
          run_hook HELIOS_INIT
          export HELIOS_CONSENSUS_RPC_URL="http://127.0.0.1:$RETHHTTP_PORT"
          # Fast-fail readiness checks before start.
          export HELIOS_READY_TIMEOUT_SECS="0"
          export HELIOS_READY_INTERVAL_SECS="1"
          if [ ! -d "$HELIOS_DIR/data" ]; then
            echo "helios data dir missing" >&2
            exit 1
          fi
          HELIOS_CHECK_OUTPUT="$(run_hook HELIOS_CHECK_CONFIG 2>&1)" || {
            echo "$HELIOS_CHECK_OUTPUT" >&2
            exit 1
          }
          echo "$HELIOS_CHECK_OUTPUT"
          HELIOS_NETWORK_VALUE="$(
            printf '%s\n' "$HELIOS_CHECK_OUTPUT" \
              | sed -n 's/.* network=\([^[:space:]]*\).*/\1/p' \
              | tail -1
          )"

          if [ "$HELIOS_NETWORK_VALUE" = "local" ]; then
            echo "Skipping HELIOS_START (network=local requires beacon consensus endpoint)" >&2
          elif nc -z 127.0.0.1 "$HELIOSRPC_PORT" >/dev/null 2>&1; then
            echo "Skipping HELIOS_START (port in use)" >&2
          else
            set +e
            run_hook HELIOS_HEALTH >/dev/null 2>&1
            HELIOS_PRE_HEALTH_RC=$?
            set -e
            if [ "$HELIOS_PRE_HEALTH_RC" -eq 0 ]; then
              echo "HELIOS_HEALTH should fail before start" >&2
              exit 1
            fi

            set +e
            run_hook HELIOS_READY >/dev/null 2>&1
            HELIOS_PRE_READY_RC=$?
            set -e
            if [ "$HELIOS_PRE_READY_RC" -eq 0 ]; then
              echo "HELIOS_READY should fail before start" >&2
              exit 1
            fi

            HELIOS_PID=$(start_service helios -- "$HELIOS_START")
            export HELIOS_READY_TIMEOUT_SECS="120"
            if ! run_hook HELIOS_READY >/dev/null 2>&1; then
              echo "helios ready check failed after start" >&2
              if [ -f "$HELIOS_DIR/logs/helios.log" ]; then
                tail -50 "$HELIOS_DIR/logs/helios.log" >&2 || true
              else
                echo "helios log missing path=$HELIOS_DIR/logs/helios.log" >&2
              fi
              run_hook HELIOS_STOP >/dev/null 2>&1 || true
              if kill -0 "$HELIOS_PID" 2>/dev/null; then
                stop_service "$HELIOS_PID" "helios"
              fi
              exit 1
            fi

            run_hook HELIOS_HEALTH >/dev/null || {
              echo "HELIOS_HEALTH should pass while running" >&2
              exit 1
            }

            run_hook HELIOS_STOP
            HELIOS_DOWN=0
            for i in $(seq 1 40); do
              if run_hook HELIOS_HEALTH >/dev/null 2>&1; then
                sleep 0.2
              else
                HELIOS_DOWN=1
                break
              fi
            done
            if [ "$HELIOS_DOWN" -ne 1 ]; then
              echo "HELIOS_HEALTH should fail after stop" >&2
              exit 1
            fi

            export HELIOS_READY_TIMEOUT_SECS="0"
            set +e
            run_hook HELIOS_READY >/dev/null 2>&1
            HELIOS_POST_READY_RC=$?
            set -e
            if [ "$HELIOS_POST_READY_RC" -eq 0 ]; then
              echo "HELIOS_READY should fail after stop" >&2
              exit 1
            fi

            if kill -0 "$HELIOS_PID" 2>/dev/null; then
              stop_service "$HELIOS_PID" "helios"
            fi
          fi
        fi

        if [ "$RETH_STARTED" -eq 1 ]; then
          run_hook RETH_STOP
          RETH_DOWN=0
          for i in $(seq 1 40); do
            if run_hook RETH_HEALTH >/dev/null 2>&1; then
              sleep 0.2
            else
              RETH_DOWN=1
              break
            fi
          done
          if [ "$RETH_DOWN" -ne 1 ]; then
            echo "RETH_HEALTH should fail after stop" >&2
            exit 1
          fi

          set +e
          run_hook RETH_READY >/dev/null 2>&1
          RETH_POST_READY_RC=$?
          set -e
          if [ "$RETH_POST_READY_RC" -eq 0 ]; then
            echo "RETH_READY should fail after stop" >&2
            exit 1
          fi

          if kill -0 "$RETH_PID" 2>/dev/null; then
            stop_service "$RETH_PID" "reth"
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
