{
  parallelRunnerOk,
  parallelRunnerFail,
}:
''

  set -euo pipefail

  fail() {
    echo "FAIL: $*" >&2
    exit 1
  }

  OK_OUT="$PWD/parallel-ok.log"
  ${parallelRunnerOk} > "$OK_OUT" 2>&1
  grep -q "first" "$OK_OUT" || fail "parallel ok output missing first"
  grep -q "second" "$OK_OUT" || fail "parallel ok output missing second"

  FAIL_OUT="$PWD/parallel-fail.log"
  set +e
  ${parallelRunnerFail} > "$FAIL_OUT" 2>&1
  RC=$?
  set -e
  [ "$RC" -ne 0 ] || fail "parallel failure run should return non-zero, got $RC"
  grep -q "failing-command" "$FAIL_OUT" || fail "parallel fail output missing failing-command"

  echo "lib parallel fixture ok"

''
