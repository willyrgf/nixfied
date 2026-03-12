{ pkgs }:
let
  shellHelpers = import ./lib/shell-helpers.nix { inherit pkgs; };
  tickerScript = pkgs.writeShellScript "supervisor-ticker" ''
    trap 'exit 0' TERM INT
    while true; do
      sleep 1
    done
  '';
  supervisorProject = {
    project = {
      id = "supervisor-lifecycle";
    };
    supervisor.services.ticker = {
      command = "${tickerScript}";
      readiness = {
        type = "exec";
        command = "true";
        initialDelaySeconds = 0;
        periodSeconds = 1;
        timeoutSeconds = 1;
        failureThreshold = 1;
      };
    };
  };

  slotsStub =
    let
      slotInfo = pkgs.writeShellScript "supervisor-slot-info" ''
        printf 'SLOT=%s\n' "''${SLOT:-0}"
        printf 'ENV=%s\n' "''${ENV:-test}"
        printf 'RUN_DIR=%s\n' "''${RUN_DIR:-/tmp}"
        printf 'LOG_DIR=%s\n' "''${LOG_DIR:-/tmp}"
        printf 'CONFIG_DIR=%s\n' "''${CONFIG_DIR:-/tmp}"
      '';
      slotInfoJson = pkgs.writeShellScript "supervisor-slot-info-json" ''
        printf '{"slot":"%s","env":"%s","directories":{"run":"%s","log":"%s","config":"%s"},"ports":{}}\n' \
          "''${SLOT:-0}" \
          "''${ENV:-test}" \
          "''${RUN_DIR:-/tmp}" \
          "''${LOG_DIR:-/tmp}" \
          "''${CONFIG_DIR:-/tmp}"
      '';
    in
    {
      getSlotInfo = slotInfo;
      getSlotInfoJson = slotInfoJson;
      getServiceDir = name: "\${RUN_DIR}/${name}";
      portVarName = key: throw "unexpected supervisor port key ${key}";
    };

  supervisor = import ../../nixfied/framework/runtime/services/supervisor/default.nix {
    inherit pkgs;
    project = supervisorProject;
    slots = slotsStub;
  };
in
pkgs.runCommand "supervisor-lifecycle-smoke" { } ''
  set -euo pipefail
  ${shellHelpers.stderrShellPrelude}

  export HOME="$TMPDIR/home"
  export SLOT=0
  export ENV=test
  export RUN_DIR="$TMPDIR/run"
  export LOG_DIR="$TMPDIR/log"
  export CONFIG_DIR="$TMPDIR/config"
  mkdir -p "$HOME" "$RUN_DIR" "$LOG_DIR" "$CONFIG_DIR"

  wait_for_success() {
    local label="$1"
    local cmd="$2"
    local log_file="$3"
    local attempts="''${4:-60}"
    local interval="''${5:-0.5}"
    local rc=0

    for _ in $(seq 1 "$attempts"); do
      set +e
      "$cmd" > "$log_file" 2>&1
      rc="$?"
      set -e
      if [ "$rc" -eq 0 ]; then
        return 0
      fi
      sleep "$interval"
    done

    cat "$log_file" >&2
    fail "$label did not succeed"
  }

  require_failure() {
    local label="$1"
    local cmd="$2"
    local log_file="$3"
    set +e
    "$cmd" > "$log_file" 2>&1
    local rc="$?"
    set -e
    if [ "$rc" -eq 0 ]; then
      cat "$log_file" >&2
      fail "$label should fail"
    fi
  }

  "${supervisor.startDaemon}" > "$TMPDIR/supervisor-start.out" 2>&1 || {
    cat "$TMPDIR/supervisor-start.out" >&2
    fail "supervisor startDaemon should succeed"
  }

  wait_for_success "supervisor health after daemon start" "${supervisor.health}" "$TMPDIR/supervisor-health.out"

  "${supervisor.status}" > "$TMPDIR/supervisor-status.out" 2>&1 || {
    cat "$TMPDIR/supervisor-status.out" >&2
    fail "supervisor status should succeed after startDaemon"
  }
  require_contains "$TMPDIR/supervisor-status.out" "ticker"

  "${supervisor.restart}" ticker > "$TMPDIR/supervisor-restart.out" 2>&1 || {
    cat "$TMPDIR/supervisor-restart.out" >&2
    fail "supervisor restart should succeed for ticker"
  }

  wait_for_success "supervisor health after restart" "${supervisor.health}" "$TMPDIR/supervisor-health-restart.out"

  "${supervisor.stop}" > "$TMPDIR/supervisor-stop.out" 2>&1 || {
    cat "$TMPDIR/supervisor-stop.out" >&2
    fail "supervisor stop should succeed"
  }

  require_failure "supervisor health after stop" "${supervisor.health}" "$TMPDIR/supervisor-health-stopped.out"

  echo "OK: supervisor lifecycle covers daemon start health restart status and stop without ready" > "$out"
''
