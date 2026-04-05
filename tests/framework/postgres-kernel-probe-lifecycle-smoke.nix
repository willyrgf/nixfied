{ pkgs }:
let
  shellHelpers = import ./lib/shell-helpers.nix { inherit pkgs; };
  slotsStub =
    let
      slotInfo = pkgs.writeShellScript "postgres-kernel-probe-slot-info" ''
        printf 'SLOT=%q\n' "''${SLOT:-0}"
        printf 'ENV=%q\n' "''${ENV:-test}"
        printf 'RUN_DIR=%q\n' "''${RUN_DIR:-/tmp}"
        printf 'LOG_DIR=%q\n' "''${LOG_DIR:-/tmp}"
        printf 'CONFIG_DIR=%q\n' "''${CONFIG_DIR:-/tmp}"
        printf 'POSTGRES_PORT=%q\n' "''${POSTGRES_PORT:-55433}"
      '';
      slotInfoJson = pkgs.writeShellScript "postgres-kernel-probe-slot-info-json" ''
        printf '{"slot":"%s","env":"%s","ports":{"POSTGRES_PORT":%s},"directories":{"run":"%s","log":"%s","config":"%s"}}\n' \
          "''${SLOT:-0}" \
          "''${ENV:-test}" \
          "''${POSTGRES_PORT:-55433}" \
          "''${RUN_DIR:-/tmp}" \
          "''${LOG_DIR:-/tmp}" \
          "''${CONFIG_DIR:-/tmp}"
      '';
    in
    {
      getSlotInfo = slotInfo;
      getSlotInfoJson = slotInfoJson;
      getServiceDir = name: "\${SERVICE_ROOT}/${name}";
      portVarName =
        key: if key == "postgres" then "POSTGRES_PORT" else throw "unsupported port key ${key}";
    };

  postgresProject = {
    project.id = "postgres-kernel-probe-lifecycle";
  };

  postgresService =
    (import ../../nixfied/modules/services/runtime/postgres/default.nix {
      inherit pkgs;
      project = postgresProject;
      slots = slotsStub;
    }).operations;
in
pkgs.runCommand "postgres-kernel-probe-lifecycle-smoke" { } ''
  set -euo pipefail
  ${shellHelpers.shellPrelude}
  ${shellHelpers.postgresCapabilityPrelude}

  skip_if_postgres_bootstrap_unavailable "postgres-kernel-probe-lifecycle-smoke"

  export SLOT=0
  export ENV=test
  export SERVICE_ROOT="$TMPDIR/service-root"
  export RUN_DIR="$TMPDIR/run"
  export LOG_DIR="$TMPDIR/log"
  export CONFIG_DIR="$TMPDIR/config"
  TEST_PORT_SEED="$(${pkgs.coreutils}/bin/printf '%s' "$TMPDIR" | ${pkgs.coreutils}/bin/cksum | ${pkgs.gawk}/bin/awk '{ print $1 }')"
  export POSTGRES_PORT="$((31000 + (TEST_PORT_SEED % 10000)))"
  mkdir -p "$SERVICE_ROOT" "$RUN_DIR" "$LOG_DIR" "$CONFIG_DIR"

  "${postgresService."full-start-test"}" > "$TMPDIR/full-start-test.out" 2>&1 || {
    cat "$TMPDIR/full-start-test.out" >&2
    fail "postgres full-start-test should succeed"
  }

  "${postgresService.status}" > "$TMPDIR/status-running.out" 2>&1 || {
    cat "$TMPDIR/status-running.out" >&2
    fail "postgres status should succeed after full-start-test"
  }
  require_contains "$TMPDIR/status-running.out" "service=postgres"
  require_contains "$TMPDIR/status-running.out" "running=true"

  "${postgresService."ready-test"}" > "$TMPDIR/ready-test.out" 2>&1 || {
    cat "$TMPDIR/ready-test.out" >&2
    fail "postgres ready-test should succeed after full-start-test"
  }
  require_contains "$TMPDIR/ready-test.out" "OK: PostgreSQL ready for test db port=$POSTGRES_PORT database="

  "${postgresService.stop}" > "$TMPDIR/stop.out" 2>&1 || {
    cat "$TMPDIR/stop.out" >&2
    fail "postgres stop should succeed after full-start-test"
  }

  if "${postgresService.status}" > "$TMPDIR/status-stopped.out" 2>&1; then
    cat "$TMPDIR/status-stopped.out" >&2
    fail "postgres status should fail after stop"
  fi
  require_contains "$TMPDIR/status-stopped.out" "running=false"

  echo "OK: postgres lifecycle probes use kernel execution for full-start-test, status, ready-test, and stop" > "$out"
''
