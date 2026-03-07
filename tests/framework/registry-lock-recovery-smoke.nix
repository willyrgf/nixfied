{
  pkgs,
  registry,
}:
let
  shellLib = registry.events.mkShellLib { };

  holder = pkgs.writeShellScript "registry-lock-holder" ''
    set -euo pipefail
    ${shellLib}

    lock_file="$1"
    ready_file="$2"
    lock_fd="$(registry_lock_acquire "$lock_file" "test-lock-holder" 5)"
    printf '%s\n' "$lock_fd" > "$ready_file"
    while true; do
      sleep 1
    done
  '';

  acquirer = pkgs.writeShellScript "registry-lock-acquirer" ''
    set -euo pipefail
    ${shellLib}

    lock_file="$1"
    lock_fd="$(registry_lock_acquire "$lock_file" "test-lock-acquirer" 5)"
    registry_lock_release "$lock_fd" "$lock_file"
    echo "OK: reacquired lock after holder termination"
  '';
in
pkgs.runCommand "registry-lock-recovery-smoke" { } ''
  set -euo pipefail

  lock_file="$TMPDIR/registry/locks/recovery.lock"
  ready_file="$TMPDIR/holder.ready"

  "${holder}" "$lock_file" "$ready_file" > "$TMPDIR/holder.out" 2>&1 &
  holder_pid="$!"

  attempt=0
  while [ ! -f "$ready_file" ] && [ "$attempt" -lt 100 ]; do
    sleep 0.1
    attempt="$((attempt + 1))"
  done

  if [ ! -f "$ready_file" ]; then
    echo "holder did not acquire lock"
    cat "$TMPDIR/holder.out"
    exit 1
  fi

  kill -9 "$holder_pid"
  wait "$holder_pid" 2>/dev/null || true

  "${acquirer}" "$lock_file" > "$TMPDIR/acquire.out" 2>&1
  ${pkgs.gnugrep}/bin/grep -Fq "OK: reacquired lock after holder termination" "$TMPDIR/acquire.out"

  echo "OK: flock-backed registry locks recover after a killed holder" > "$out"
''
