{
  pkgs,
  model,
  services,
  serviceDefinitions,
  registry,
}:
let
  harness = import ./lib/harness.nix {
    inherit
      pkgs
      model
      services
      serviceDefinitions
      registry
      ;
    projectRoot = ../..;
  };
in
pkgs.runCommand "framework-test-cli-contract-smoke" { } ''
    set -euo pipefail
    ${harness.shellPrelude}

    ORCH="${harness.orchestrator}/bin/nixfied-orchestrator"
    export REGISTRY_ROOT="$TMPDIR/registry"
    export CI_ARTIFACTS_DIR="$TMPDIR/artifacts"
    export HOME="$TMPDIR/home"
    mkdir -p "$REGISTRY_ROOT" "$CI_ARTIFACTS_DIR" "$HOME"

    "$ORCH" run-task task.framework.test --list-shards > "$TMPDIR/list-shards.out" 2>&1
    require_contains "$TMPDIR/list-shards.out" "compile"
    require_contains "$TMPDIR/list-shards.out" "manifest"
    require_contains "$TMPDIR/list-shards.out" "kernel"
    require_contains "$TMPDIR/list-shards.out" "adapters"
    require_contains "$TMPDIR/list-shards.out" "e2e"
    require_contains "$TMPDIR/list-shards.out" "migration"
    require_not_contains "$TMPDIR/list-shards.out" "services"

    "$ORCH" run-task task.framework.test --profile feature-proof --serial --summary > "$TMPDIR/profile-feature-proof.out" 2>&1
    require_contains "$TMPDIR/profile-feature-proof.out" "INFO: running shards serial total=4"
    require_contains "$TMPDIR/profile-feature-proof.out" "OK: shard passed name=compile"
    require_contains "$TMPDIR/profile-feature-proof.out" "OK: shard passed name=manifest"
    require_contains "$TMPDIR/profile-feature-proof.out" "OK: shard passed name=adapters"
    require_contains "$TMPDIR/profile-feature-proof.out" "OK: shard passed name=e2e"
    require_contains "$TMPDIR/profile-feature-proof.out" "INFO: summary profile=feature-proof executed_shards=4 failed_shards=0 exit_1_shards=0 canceled_shards=0"

    "$ORCH" run-task task.framework.test --shard manifest --summary > "$TMPDIR/shard-manifest.out" 2>&1
    require_contains "$TMPDIR/shard-manifest.out" "OK: shard passed name=manifest"
    require_contains "$TMPDIR/shard-manifest.out" "INFO: summary profile=ci executed_shards=1 failed_shards=0 exit_1_shards=0 canceled_shards=0"

    "$ORCH" run-task task.framework.test --shard manifest --summary --log-level debug --output-mode both > "$TMPDIR/shard-manifest-logging.out" 2>&1
    require_contains "$TMPDIR/shard-manifest-logging.out" "OK: shard passed name=manifest"

    "$ORCH" run-task task.framework.test --shard manifest --serial --summary > "$TMPDIR/shard-manifest-serial.out" 2>&1
    require_contains "$TMPDIR/shard-manifest-serial.out" "INFO: running shards serial total=1"
    require_contains "$TMPDIR/shard-manifest-serial.out" "OK: shard passed name=manifest"

    "$ORCH" run-task task.framework.test --shard manifest --max-parallel-shards auto --summary > "$TMPDIR/shard-manifest-auto.out" 2>&1
    require_contains "$TMPDIR/shard-manifest-auto.out" "OK: shard passed name=manifest"

    summary_json="$TMPDIR/framework-summary.json"
    "$ORCH" run-task task.framework.test --profile full --shard manifest --summary-json "$summary_json" > "$TMPDIR/summary-json.out" 2>&1
    require_file "$summary_json"
    ${pkgs.jq}/bin/jq -e '
      .profile == "full"
      and .shard == "manifest"
      and .executed_shards == 1
      and .failed_shards == 0
      and .exit_1_shards == 0
      and .canceled_shards == 0
      and .exit_code == 0
      and (.duration_seconds | type == "number")
      and (.started_at | type == "string")
      and (.finished_at | type == "string")
    ' "$summary_json" > /dev/null

    set +e
    "$ORCH" run-task task.framework.test --profile nope > "$TMPDIR/profile-nope.out" 2>&1
    profile_rc="$?"
    set -e
    if [ "$profile_rc" -eq 0 ]; then
      fail "expected unknown profile to fail"
    fi
    require_contains "$TMPDIR/profile-nope.out" "unknown profile 'nope'"

    set +e
    "$ORCH" run-task task.framework.test --shard unknown > "$TMPDIR/shard-unknown.out" 2>&1
    shard_rc="$?"
    set -e
    if [ "$shard_rc" -eq 0 ]; then
      fail "expected unknown shard to fail"
    fi
    require_contains "$TMPDIR/shard-unknown.out" "unknown shard 'unknown'"

    set +e
    "$ORCH" run-task task.framework.test --shard manifest --max-parallel-shards 0 > "$TMPDIR/max-parallel-invalid.out" 2>&1
    max_parallel_rc="$?"
    set -e
    if [ "$max_parallel_rc" -eq 0 ]; then
      fail "expected --max-parallel-shards 0 to fail"
    fi
    require_contains "$TMPDIR/max-parallel-invalid.out" "invalid --max-parallel-shards '0'"

  set +e
  NIXFIED_FRAMEWORK_TEST_FORCE_FAIL_SHARD=manifest \
    "$ORCH" run-task task.framework.test --shard manifest > "$TMPDIR/failing-shard.out" 2>&1
  failing_shard_rc="$?"
  set -e
  if [ "$failing_shard_rc" -ne 17 ]; then
    fail "expected forced manifest shard to exit 17"
  fi
  require_contains "$TMPDIR/failing-shard.out" "ERROR: shard failed name=manifest rc=17"
  require_contains "$TMPDIR/failing-shard.out" "INFO: summary profile=ci executed_shards=0 failed_shards=1 exit_1_shards=0 canceled_shards=0"

    echo "OK: framework::test CLI contract is validated" > "$out"
''
