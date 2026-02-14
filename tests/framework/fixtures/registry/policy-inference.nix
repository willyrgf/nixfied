{
  emitEvent,
  serviceStatus,
  processInspect,
  registryRoot,
  ephemeralFlagVar,
}:
''
  set -euo pipefail

  fail() {
    echo "FAIL: $*" >&2
    exit 1
  }

  assert_service_status() {
    local expected_scope="$1"
    local expected_state="$2"
    local out=""

    out="$(${serviceStatus} --service policy-fixture --slot 0 --env dev)"
    eval "$out"

    if [ "$REGISTRY_SCOPE" != "$expected_scope" ]; then
      echo "$out" >&2
      fail "expected REGISTRY_SCOPE=$expected_scope, got $REGISTRY_SCOPE"
    fi

    if [ "$REGISTRY_STATE" != "$expected_state" ]; then
      echo "$out" >&2
      fail "expected REGISTRY_STATE=$expected_state, got $REGISTRY_STATE"
    fi
  }

  assert_event_policy() {
    local run_id="$1"
    local reuse_policy="$2"
    local owner_scope="$3"
    local discovery_scope="$4"
    local inspect_out=""

    ${emitEvent} \
      --event-type run_started \
      --run-id "$run_id" \
      --state running \
      --slot 0 \
      --env dev >/dev/null

    inspect_out="$(${processInspect} --id "$run_id")"
    if ! printf '%s\n' "$inspect_out" | grep -q "\"reuse_policy\": \"$reuse_policy\""; then
      echo "$inspect_out" >&2
      fail "expected reuse_policy=$reuse_policy for run_id=$run_id"
    fi
    if ! printf '%s\n' "$inspect_out" | grep -q "\"owner_scope\": \"$owner_scope\""; then
      echo "$inspect_out" >&2
      fail "expected owner_scope=$owner_scope for run_id=$run_id"
    fi
    if ! printf '%s\n' "$inspect_out" | grep -q "\"discovery_scope\": \"$discovery_scope\""; then
      echo "$inspect_out" >&2
      fail "expected discovery_scope=$discovery_scope for run_id=$run_id"
    fi
  }

  rm -rf "${registryRoot}"
  mkdir -p "${registryRoot}"

  export ${ephemeralFlagVar}=1

  unset SERVICE_REUSE_POLICY SERVICE_OWNER_SCOPE SERVICE_DISCOVERY_SCOPE
  assert_service_status "local" "local_only"

  export SERVICE_REUSE_POLICY="same-slot"
  unset SERVICE_OWNER_SCOPE SERVICE_DISCOVERY_SCOPE
  assert_service_status "global" "unknown"
  assert_event_policy "policy-same-slot" "same-slot" "persistent" "global"

  export SERVICE_REUSE_POLICY="same-slot"
  unset SERVICE_OWNER_SCOPE
  export SERVICE_DISCOVERY_SCOPE="local"
  assert_service_status "local" "local_only"

  export SERVICE_REUSE_POLICY="cross-run"
  unset SERVICE_OWNER_SCOPE SERVICE_DISCOVERY_SCOPE
  assert_service_status "global" "unknown"
  assert_event_policy "policy-cross-run" "cross-run" "persistent" "global"

  export SERVICE_REUSE_POLICY="same-root"
  unset SERVICE_OWNER_SCOPE SERVICE_DISCOVERY_SCOPE
  assert_service_status "local" "local_only"
  assert_event_policy "policy-same-root" "same-root" "ephemeral" "local"

  echo "registry policy inference fixture ok"
''
