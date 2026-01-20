# PostgreSQL module (optional)
{
  pkgs,
  project,
  slots,
}:

let
  cfg = project.modules.postgres or { };
  postgres = cfg.package or pkgs.postgresql_16;
  portKey = cfg.portKey or "postgres";
  portVar = slots.portVarName portKey;
  dataDirName = cfg.dataDirName or "postgres";
  pgdataExpr = slots.getServiceDir dataDirName;
  database = cfg.database or "app";
  testDatabase = cfg.testDatabase or "${database}_test";
  extensions = cfg.extensions or [ ];
  extraConfig = cfg.extraConfig or "";

  baseConf = ''
    listen_addresses = 'localhost'
    port = $PGPORT
    unix_socket_directories = '/tmp'
    max_connections = 50
    shared_buffers = 128MB
    log_destination = 'stderr'
    logging_collector = off
  '';

  init = pkgs.writeShellScript "postgres-init" ''
            set -euo pipefail

            if [ -z "''${PGDATA:-}" ] || [ -z "''${PGPORT:-}" ]; then
              echo "❌ PGDATA and PGPORT must be set" >&2
              exit 1
            fi

            mkdir -p "$PGDATA"

            if [ -f "$PGDATA/PG_VERSION" ]; then
              echo "✅ PostgreSQL already initialized at $PGDATA"
              exit 0
            fi

            echo "🔧 Initializing PostgreSQL at $PGDATA..."
            ${postgres}/bin/initdb -D "$PGDATA" -U postgres --no-locale --encoding=UTF8 -A trust

            cat > "$PGDATA/postgresql.conf" <<'EOF'
    ${baseConf}
    ${extraConfig}
    EOF

            cat > "$PGDATA/pg_hba.conf" <<'EOF'
    # TYPE  DATABASE        USER  ADDRESS       METHOD
    local   all             all                 trust
    host    all             all   127.0.0.1/32  trust
    host    all             all   ::1/128       trust
    EOF
  '';

  start = pkgs.writeShellScript "postgres-start" ''
    set -euo pipefail

    if [ -z "''${PGDATA:-}" ] || [ -z "''${PGPORT:-}" ]; then
      echo "❌ PGDATA and PGPORT must be set" >&2
      exit 1
    fi

    if ${postgres}/bin/pg_isready -U postgres -h localhost -p "$PGPORT" -q 2>/dev/null; then
      echo "✅ PostgreSQL already running on port $PGPORT"
      exit 0
    fi

    echo "🚀 Starting PostgreSQL on port $PGPORT..."
    ${postgres}/bin/pg_ctl -D "$PGDATA" -l "$PGDATA/postgres.log" -o "-p $PGPORT" start

    for i in $(seq 1 60); do
      if ${postgres}/bin/pg_isready -U postgres -h localhost -p "$PGPORT" -q 2>/dev/null; then
        echo "✅ PostgreSQL ready on port $PGPORT"
        exit 0
      fi
      sleep 0.5
    done

    echo "❌ PostgreSQL failed to start. Check $PGDATA/postgres.log"
    tail -20 "$PGDATA/postgres.log" || true
    exit 1
  '';

  stop = pkgs.writeShellScript "postgres-stop" ''
    if [ -n "''${PGDATA:-}" ] && [ -f "$PGDATA/postmaster.pid" ]; then
      echo "🛑 Stopping PostgreSQL at $PGDATA..."
      ${postgres}/bin/pg_ctl -D "$PGDATA" stop -m fast 2>/dev/null || true
    fi
  '';

  setupDb = pkgs.writeShellScript "postgres-setup-db" ''
    set -euo pipefail

    if [ -z "''${PGPORT:-}" ] || [ -z "''${PGDATABASE:-}" ]; then
      echo "❌ PGPORT and PGDATABASE must be set" >&2
      exit 1
    fi

    echo "📦 Setting up database '$PGDATABASE'..."

    ${postgres}/bin/psql -h localhost -p "$PGPORT" -U postgres -d postgres -c \
      "DO \$\$ BEGIN CREATE ROLE postgres WITH LOGIN SUPERUSER PASSWORD 'postgres'; EXCEPTION WHEN duplicate_object THEN NULL; END \$\$;" 2>/dev/null || true

    ${postgres}/bin/createdb -h localhost -p "$PGPORT" -U postgres "$PGDATABASE" 2>/dev/null || true

    if [ -n "${pkgs.lib.concatStringsSep " " extensions}" ]; then
      for ext in ${pkgs.lib.concatStringsSep " " extensions}; do
        ${postgres}/bin/psql -h localhost -p "$PGPORT" -U postgres -d "$PGDATABASE" \
          -c "CREATE EXTENSION IF NOT EXISTS $ext;" 2>/dev/null || true
      done
    fi

    echo "✅ Database '$PGDATABASE' ready"
  '';

  fullStart = pkgs.writeShellScript "postgres-full-start" ''
    set -euo pipefail
    eval "$(${slots.getSlotInfo})"

    PORT_VAR="${portVar}"
    export PGPORT="''${!PORT_VAR}"
    export PGDATA="${pgdataExpr}"
    export PGDATABASE="''${PGDATABASE:-${database}}"

    echo "🎰 Slot $SLOT, env $ENV (PGPORT=$PGPORT)"

    ${init}
    ${start}
    ${setupDb}

    echo "PGPORT=$PGPORT"
    echo "PGDATA=$PGDATA"
    echo "PGDATABASE=$PGDATABASE"
  '';

  fullStartTest = pkgs.writeShellScript "postgres-full-start-test" ''
    set -euo pipefail
    eval "$(${slots.getSlotInfo})"

    PORT_VAR="${portVar}"
    export PGPORT="''${!PORT_VAR}"
    export PGDATA="${pgdataExpr}"
    export PGDATABASE="''${PGDATABASE:-${testDatabase}}"

    ${init}
    ${start}
    ${setupDb}
  '';

in
{
  inherit
    postgres
    init
    start
    stop
    setupDb
    fullStart
    fullStartTest
    ;
}
