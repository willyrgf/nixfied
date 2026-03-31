{ pkgs }:
let
  shellHelpers = import ./lib/shell-helpers.nix { inherit pkgs; };
  slotInfo = pkgs.writeShellScript "postgres-config-artifacts-slot-info-shell" ''
    printf 'SLOT=%q\n' "''${SLOT:-0}"
    printf 'ENV=%q\n' "''${ENV:-dev}"
    printf 'RUN_DIR=%q\n' "''${RUN_DIR:-/tmp}"
    printf 'POSTGRES_PORT=%q\n' "''${POSTGRES_PORT:-55433}"
  '';
  slotInfoJson = pkgs.writeShellScript "postgres-config-artifacts-slot-info" ''
    printf '{"slot":"%s","env":"%s","ports":{"POSTGRES_PORT":%s},"directories":{"run":"%s"}}\n' \
      "''${SLOT:-0}" \
      "''${ENV:-dev}" \
      "''${POSTGRES_PORT:-55433}" \
      "''${RUN_DIR:-/tmp}"
  '';

  slotsStub = {
    getSlotInfo = slotInfo;
    getSlotInfoJson = slotInfoJson;
    getServiceDir = name: "\${PGDATA_ROOT}/${name}";
    portVarName = _: "POSTGRES_PORT";
  };

  project = {
    project = {
      id = "postgres-config-artifacts";
    };
  };

  config = import ../../nixfied/modules/services/runtime/postgres/config.nix {
    inherit pkgs project;
  };

  lifecycle = import ../../nixfied/modules/services/runtime/postgres/lifecycle.nix {
    inherit
      pkgs
      project
      config
      ;
    slots = slotsStub;
    loggingPrelude = shellHelpers.loggingPreludeWithStop;
  };
in
pkgs.runCommand "postgres-config-artifacts-smoke" { } ''
  set -euo pipefail
  ${shellHelpers.stderrShellPrelude}
  ${shellHelpers.postgresCapabilityPrelude}

  export HOME="$TMPDIR/home"
  export SLOT=0
  export ENV=test
  export RUN_DIR="$TMPDIR/run"
  export PGDATA_ROOT="$TMPDIR/service-root"
  export POSTGRES_PORT=55434

  mkdir -p "$HOME" "$RUN_DIR" "$PGDATA_ROOT"
  skip_if_postgres_bootstrap_unavailable "postgres-config-artifacts-smoke"

  "${lifecycle.init}" > "$TMPDIR/init.out" 2>&1 || {
    cat "$TMPDIR/init.out" >&2
    fail "postgres init should succeed with compiled config artifacts"
  }

  "${lifecycle.checkConfig}" > "$TMPDIR/check-config.out" 2>&1 || {
    cat "$TMPDIR/check-config.out" >&2
    fail "postgres check-config should accept installed config artifacts"
  }

  require_contains "$PGDATA_ROOT/postgres/postgresql.conf" "port = $POSTGRES_PORT"
  require_contains "$PGDATA_ROOT/postgres/postgresql.conf" "autovacuum = off"
  require_contains "$PGDATA_ROOT/postgres/pg_hba.conf" "127.0.0.1/32  trust"
  require_contains "$TMPDIR/check-config.out" "OK: PostgreSQL configuration valid pgdata=$PGDATA_ROOT/postgres"

  echo "OK: postgres init and check-config consume compiled config artifacts" > "$out"
''
