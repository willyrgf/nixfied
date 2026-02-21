{
  pkgs,
  model,
  registry,
}:
let
  harness = import ./lib/harness.nix {
    inherit
      pkgs
      model
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
  require_contains "$TMPDIR/list-shards.out" "flake-check"
  require_contains "$TMPDIR/list-shards.out" "help"
  require_contains "$TMPDIR/list-shards.out" "workflow-test"
  require_contains "$TMPDIR/list-shards.out" "workflow-ci"
  require_contains "$TMPDIR/list-shards.out" "isolation"
  require_contains "$TMPDIR/list-shards.out" "self-host"

  "$ORCH" run-task task.framework.test --shard help --summary > "$TMPDIR/shard-help.out" 2>&1
  require_contains "$TMPDIR/shard-help.out" "OK: shard passed name=help"
  require_contains "$TMPDIR/shard-help.out" "INFO: summary profile=ci mode=full executed_shards=1"

  "$ORCH" run-task task.framework.test --shard help --summary --log-level debug --output-mode both > "$TMPDIR/shard-help-logging.out" 2>&1
  require_contains "$TMPDIR/shard-help-logging.out" "OK: shard passed name=help"

  "$ORCH" run-task task.framework.test --shard help --serial --summary > "$TMPDIR/shard-help-serial.out" 2>&1
  require_contains "$TMPDIR/shard-help-serial.out" "INFO: running shards serial total=1"
  require_contains "$TMPDIR/shard-help-serial.out" "OK: shard passed name=help"

  "$ORCH" run-task task.framework.test --shard help --max-parallel-shards auto --summary > "$TMPDIR/shard-help-auto.out" 2>&1
  require_contains "$TMPDIR/shard-help-auto.out" "OK: shard passed name=help"

  summary_json="$TMPDIR/framework-summary.json"
  "$ORCH" run-task task.framework.test --shard help --summary-json "$summary_json" > "$TMPDIR/summary-json.out" 2>&1
  require_file "$summary_json"
  ${pkgs.jq}/bin/jq -e '
    .profile == "ci"
    and .mode == "full"
    and .shard == "help"
    and .executed_shards == 1
    and .exit_code == 0
    and (.duration_seconds | type == "number")
    and (.started_at | type == "string")
    and (.finished_at | type == "string")
  ' "$summary_json" > /dev/null

  set +e
  "$ORCH" run-task task.framework.test --profile full > "$TMPDIR/profile-full.out" 2>&1
  profile_rc="$?"
  set -e
  if [ "$profile_rc" -eq 0 ]; then
    fail "expected --profile full to fail"
  fi
  require_contains "$TMPDIR/profile-full.out" "profile 'full' is no longer supported"

  set +e
  "$ORCH" run-task task.framework.test --mode nope --shard help > "$TMPDIR/mode-nope.out" 2>&1
  mode_rc="$?"
  set -e
  if [ "$mode_rc" -eq 0 ]; then
    fail "expected unknown mode to fail"
  fi
  require_contains "$TMPDIR/mode-nope.out" "unknown mode 'nope'"

  set +e
  "$ORCH" run-task task.framework.test --shard unknown > "$TMPDIR/shard-unknown.out" 2>&1
  shard_rc="$?"
  set -e
  if [ "$shard_rc" -eq 0 ]; then
    fail "expected unknown shard to fail"
  fi
  require_contains "$TMPDIR/shard-unknown.out" "unknown shard 'unknown'"

  set +e
  "$ORCH" run-task task.framework.test --shard help --max-parallel-shards 0 > "$TMPDIR/max-parallel-invalid.out" 2>&1
  max_parallel_rc="$?"
  set -e
  if [ "$max_parallel_rc" -eq 0 ]; then
    fail "expected --max-parallel-shards 0 to fail"
  fi
  require_contains "$TMPDIR/max-parallel-invalid.out" "invalid --max-parallel-shards '0'"

  echo "OK: framework::test CLI contract is validated" > "$out"
''
