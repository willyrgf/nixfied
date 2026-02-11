{
  runRegistryStart,
  runsRoot,
}:
''

  set -euo pipefail

  fail() {
    echo "FAIL: $*" >&2
    exit 1
  }

  mkdir -p "${runsRoot}"

  ${runRegistryStart} --name "bg-fixture" --script "echo bg-start; sleep 1; echo bg-done" --bg >/dev/null

  BG_RUN_DIR=""
  for _ in $(seq 1 50); do
    BG_RUN_DIR=$(ls -td "${runsRoot}"/*/ 2>/dev/null | head -1 || true)
    if [ -n "$BG_RUN_DIR" ] && grep -q '"status": "passed"' "$BG_RUN_DIR/meta.json" 2>/dev/null; then
      break
    fi
    sleep 0.2
  done

  [ -n "$BG_RUN_DIR" ] || fail "no background run directory created"
  [ -f "$BG_RUN_DIR/meta.json" ] || fail "meta.json missing for background run"
  grep -q '"status": "passed"' "$BG_RUN_DIR/meta.json" || fail "background run did not pass"
  grep -q "bg-done" "$BG_RUN_DIR/output.log" || fail "background output missing"

  set +e
  ${runRegistryStart} --name "timeout-fixture" --script "sleep 5" --timeout 1 >/dev/null 2>&1
  RC=$?
  set -e
  [ "$RC" -ne 0 ] || fail "timeout run should fail"

  TIMEOUT_RUN_DIR=$(ls -td "${runsRoot}"/*/ 2>/dev/null | head -1 || true)
  [ -n "$TIMEOUT_RUN_DIR" ] || fail "timeout run directory missing"
  [ -f "$TIMEOUT_RUN_DIR/meta.json" ] || fail "timeout run meta missing"

  echo "registry background+timeout fixture ok"

''
