# Run registry foreground test fixture
{
  runRegistryStart,
  runsRoot,
}:
''
  fail() {
    echo "FAIL: $*" >&2
    exit 1
  }

  # Test 1: foreground run creates meta.json and output.log
  set +e
  REG_OUTPUT=$(${runRegistryStart} \
    --name "test-fg" \
    --script "echo hello-from-registry" 2>&1)
  RC=$?
  set -e

  if [ "$RC" -ne 0 ]; then
    echo "$REG_OUTPUT" >&2
    fail "foreground run exited non-zero: $RC"
  fi

  # Find the most recent run directory
  RUN_DIR=$(ls -td "${runsRoot}"/*/ 2>/dev/null | head -1)
  if [ -z "$RUN_DIR" ]; then
    fail "no run directory created in ${runsRoot}"
  fi

  # Test 2: meta.json exists
  if [ ! -f "$RUN_DIR/meta.json" ]; then
    fail "expected meta.json in $RUN_DIR"
  fi

  # Test 3: output.log exists and has content
  if [ ! -f "$RUN_DIR/output.log" ]; then
    fail "expected output.log in $RUN_DIR"
  fi
  if ! grep -q "hello-from-registry" "$RUN_DIR/output.log"; then
    fail "expected output to contain hello-from-registry"
  fi

  # Test 4: meta.json has required fields
  if ! grep -q '"status"' "$RUN_DIR/meta.json"; then
    fail "meta.json missing status field"
  fi
  if ! grep -q '"run_id"' "$RUN_DIR/meta.json"; then
    fail "meta.json missing run_id field"
  fi
  if ! grep -q '"target"' "$RUN_DIR/meta.json"; then
    fail "meta.json missing target field"
  fi
  if ! grep -q '"passed"' "$RUN_DIR/meta.json"; then
    echo "meta.json content:" >&2
    cat "$RUN_DIR/meta.json" >&2
    fail "meta.json should show passed status"
  fi

  # Cleanup test run
  rm -rf "$RUN_DIR"

  echo "registry foreground tests passed"
''
