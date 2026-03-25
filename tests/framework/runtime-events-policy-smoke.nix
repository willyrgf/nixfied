{ pkgs }:
let
  shellHelpers = import ./lib/shell-helpers.nix { inherit pkgs; };
  runtimeEvents = import ../../nixfied/framework/runtime/helpers/runtime-events.nix {
    inherit pkgs;
    project = {
      project.id = "runtime-events-policy-smoke";
    };
  };
in
pkgs.runCommand "runtime-events-policy-smoke" { } ''
  set -euo pipefail
  ${shellHelpers.shellPrelude}

  export REGISTRY_ROOT="$TMPDIR/registry"

  export RUNTIME_EVENTS_POLICY_SMOKE_EPHEMERAL=1
  STATUS_OUT="$("${runtimeEvents.serviceStatus}" --service postgres --slot 0 --env test)"
  eval "$STATUS_OUT"
  [ "$REGISTRY_SCOPE" = "local" ] || fail "expected local registry scope for ephemeral runtime policy"
  [ "$REGISTRY_FOUND" = "0" ] || fail "expected no registry lookup for local discovery scope"
  [ "$REGISTRY_STATE" = "local_only" ] || fail "expected local_only registry state for local discovery scope"

  "${runtimeEvents.emitEvent}" --event-type service_ready --service postgres --run-id local-run --slot 0 --env test > /dev/null
  LOCAL_EVENT="$("${runtimeEvents.serviceEvents}" --service postgres --slot 0 --env test --limit 1)"
  printf '%s' "$LOCAL_EVENT" | ${pkgs.gnugrep}/bin/grep -F '"ownerScope":"ephemeral"' > /dev/null \
    || fail "expected emitted event to derive ownerScope=ephemeral from ephemeral mode"
  printf '%s' "$LOCAL_EVENT" | ${pkgs.gnugrep}/bin/grep -F '"discoveryScope":"local"' > /dev/null \
    || fail "expected emitted event to derive discoveryScope=local from ephemeral mode"
  printf '%s' "$LOCAL_EVENT" | ${pkgs.gnugrep}/bin/grep -F '"reusePolicy":"same-root"' > /dev/null \
    || fail "expected emitted event to derive reusePolicy=same-root from ephemeral mode"

  unset RUNTIME_EVENTS_POLICY_SMOKE_EPHEMERAL
  export SERVICE_REUSE_POLICY="same-slot"
  "${runtimeEvents.emitEvent}" --event-type service_ready --service reth --run-id global-run --slot 0 --env test > /dev/null
  GLOBAL_EVENT="$("${runtimeEvents.serviceEvents}" --service reth --slot 0 --env test --limit 1)"
  printf '%s' "$GLOBAL_EVENT" | ${pkgs.gnugrep}/bin/grep -F '"ownerScope":"persistent"' > /dev/null \
    || fail "expected emitted event to derive ownerScope=persistent from same-slot reuse"
  printf '%s' "$GLOBAL_EVENT" | ${pkgs.gnugrep}/bin/grep -F '"discoveryScope":"global"' > /dev/null \
    || fail "expected emitted event to derive discoveryScope=global from same-slot reuse"
  printf '%s' "$GLOBAL_EVENT" | ${pkgs.gnugrep}/bin/grep -F '"reusePolicy":"same-slot"' > /dev/null \
    || fail "expected emitted event to keep same-slot reuse policy"
  unset SERVICE_REUSE_POLICY

  if SERVICE_REUSE_POLICY=cross-run \
    "${runtimeEvents.emitEvent}" \
      --event-type service_ready \
      --service broken \
      --run-id broken-run \
      --slot 0 \
      --env test \
      --owner-scope ephemeral > "$TMPDIR/invalid-policy.log" 2>&1; then
    fail "expected cross-run reuse with owner-scope=ephemeral to fail validation"
  fi
  if [ -f "$REGISTRY_ROOT/events.ndjson" ] && ${pkgs.gnugrep}/bin/grep -F '"runId":"broken-run"' "$REGISTRY_ROOT/events.ndjson" > /dev/null; then
    fail "invalid runtime event policy should not append registry events"
  fi

  echo "OK: runtime event policy resolution is kernel-owned and behaviorally enforced" > "$out"
''
