{ pkgs }:
let
  slotInfoJson = pkgs.writeShellScript "postgres-config-artifacts-slot-info" ''
    printf '{"slot":"%s","env":"%s","ports":{"POSTGRES_PORT":%s},"directories":{"run":"%s"}}\n' \
      "''${SLOT:-0}" \
      "''${ENV:-dev}" \
      "''${POSTGRES_PORT:-55433}" \
      "''${RUN_DIR:-/tmp}"
  '';

  slotsStub = {
    getSlotInfoJson = slotInfoJson;
    getServiceDir = name: "\${PGDATA_ROOT}/${name}";
    portVarName = _: "POSTGRES_PORT";
  };

  project = {
    project = {
      id = "postgres-config-artifacts";
    };
  };

  config = import ../../nixfied/.framework/postgres/config.nix {
    inherit pkgs project;
  };

  lifecycle = import ../../nixfied/.framework/postgres/lifecycle.nix {
    inherit
      pkgs
      project
      config
      ;
    slots = slotsStub;
    loggingPrelude = ''
      log_info() { printf 'INFO: %s\n' "$*"; }
      log_warn() { printf 'WARN: %s\n' "$*"; }
      log_error() { printf 'ERROR: %s\n' "$*"; }
      log_ok() { printf 'OK: %s\n' "$*"; }
      log_stop() { printf 'INFO: stopping %s\n' "$*"; }
      log_skip() { printf 'SKIP: %s\n' "$*"; }
    '';
  };
in
pkgs.runCommand "postgres-config-artifacts-smoke" { } ''
  set -euo pipefail

  fail() {
    echo "ERROR: $*" >&2
    exit 1
  }

  require_contains() {
    local path="$1"
    local needle="$2"
    ${pkgs.gnugrep}/bin/grep -Fq -- "$needle" "$path" || {
      echo "--- $path" >&2
      cat "$path" >&2
      fail "expected '$needle' in $path"
    }
  }

  export HOME="$TMPDIR/home"
  export SLOT=0
  export ENV=test
  export RUN_DIR="$TMPDIR/run"
  export PGDATA_ROOT="$TMPDIR/service-root"
  export POSTGRES_PORT=55433

  mkdir -p "$HOME" "$RUN_DIR" "$PGDATA_ROOT"

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
