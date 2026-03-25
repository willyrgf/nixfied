{ pkgs }:
let
  shellHelpers = import ./lib/shell-helpers.nix { inherit pkgs; };
  helpers = import ../../nixfied/framework/runtime/helpers/helpers.nix {
    inherit pkgs;
    hooks = { };
    summaryParser = "";
  };
  kernelPackage = import ../../nixfied/framework/runtime/kernel { inherit pkgs; };
in
pkgs.runCommand "service-policy-runtime-smoke" { } ''
  set -euo pipefail
  ${shellHelpers.shellPrelude}

  . ${helpers.helpersScript}

  resolve_fixture_keep_running() {
    local owner_scope="''${1:-}"
    local reuse_policy="''${2:-}"
    local discovery_scope="''${3:-}"
    local export_file=""

    export_file="$(mktemp "''${TMPDIR:-/tmp}/nixfied-fixture-policy-test.XXXXXX")" || return 1
    if ! ${kernelPackage}/bin/nixfied-kernel service-policy fixture-keep-running \
      "$owner_scope" \
      "$reuse_policy" \
      "$discovery_scope" \
      "$export_file" >/dev/null; then
      rm -f "$export_file"
      return 1
    fi
    . "$export_file"
    rm -f "$export_file"
    printf '%s' "$KEEP_RUNNING"
  }

  [ "$(start_service_should_register_cleanup auto)" = "1" ] \
    || fail "expected start_service auto cleanup when no policy env is set"

  SERVICE_REUSE_POLICY="same-slot"
  [ "$(start_service_should_register_cleanup auto)" = "0" ] \
    || fail "expected start_service same-slot policy to preserve the process"
  unset SERVICE_REUSE_POLICY

  SERVICE_DISCOVERY_SCOPE="global"
  [ "$(start_service_should_register_cleanup auto)" = "0" ] \
    || fail "expected start_service global discovery to preserve the process"
  unset SERVICE_DISCOVERY_SCOPE

  SERVICE_OWNER_SCOPE="ephemeral"
  [ "$(start_service_should_register_cleanup auto)" = "1" ] \
    || fail "expected start_service ephemeral owner to keep cleanup enabled"
  unset SERVICE_OWNER_SCOPE

  NIXFIED_START_SERVICE_MANAGED_CLEANUP=1
  SERVICE_OWNER_SCOPE="persistent"
  [ "$(start_service_should_register_cleanup auto)" = "0" ] \
    || fail "expected managed cleanup mode to suppress helper cleanup registration"
  unset NIXFIED_START_SERVICE_MANAGED_CLEANUP SERVICE_OWNER_SCOPE

  SERVICE_REUSE_POLICY="cross-run"
  SERVICE_OWNER_SCOPE="ephemeral"
  if start_service_should_register_cleanup auto > "$TMPDIR/start-policy-invalid.log" 2>&1; then
    fail "expected invalid cross-run start_service policy matrix to fail"
  fi
  unset SERVICE_REUSE_POLICY SERVICE_OWNER_SCOPE

  [ "$(resolve_fixture_keep_running "" "" "")" = "0" ] \
    || fail "expected fixtures to keep default keep_running=0"

  [ "$(resolve_fixture_keep_running "persistent" "" "")" = "1" ] \
    || fail "expected persistent fixture owner scope to force keep_running=1"

  [ "$(resolve_fixture_keep_running "" "same-root" "")" = "0" ] \
    || fail "expected same-root fixture reuse to keep keep_running=0"

  [ "$(resolve_fixture_keep_running "" "" "global")" = "1" ] \
    || fail "expected global fixture discovery to force keep_running=1"

  if resolve_fixture_keep_running "bogus" "" "" > "$TMPDIR/fixture-policy-invalid.log" 2>&1; then
    fail "expected invalid fixture owner scope to fail validation"
  fi

  echo "OK: start-service cleanup and fixture keep_running policy are kernel-owned" > "$out"
''
