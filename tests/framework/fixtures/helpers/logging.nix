{ }:
''
  set -euo pipefail

  fail() {
    echo "FAIL: $*" >&2
    exit 1
  }

  assert_contains() {
    local file="$1"
    local pattern="$2"
    grep -q -- "$pattern" "$file" || fail "expected '$pattern' in $file"
  }

  assert_not_contains() {
    local file="$1"
    local pattern="$2"
    if grep -q -- "$pattern" "$file"; then
      fail "unexpected '$pattern' in $file"
    fi
  }

  assert_empty() {
    local file="$1"
    if [ -s "$file" ]; then
      fail "expected empty file: $file"
    fi
  }

  TMPDIR=$(mktemp -d)
  cleanup() {
    rm -rf "$TMPDIR"
  }
  trap cleanup EXIT

  run_trace_probe() {
    local trace_file="$1"
    local level="$2"
    local trace_flag="$3"

    (
      set -euo pipefail
      export LOG_LEVEL="$level"
      export NIXFIED_LOG_TRACE="$trace_flag"
      export LOG_DIR="$TMPDIR"
      export COMMAND_NAME="logging-fixture"
      export PROJECT_ENV="test"
      export NIX_ENV="0"

      if [ "''${LOG_LEVEL:-}" = "trace" ] && [ "''${NIXFIED_LOG_TRACE:-0}" = "1" ]; then
        if [ -z "''${NIXFIED_XTRACE_FILE:-}" ]; then
          NIXFIED_XTRACE_FILE="$trace_file"
          export NIXFIED_XTRACE_FILE
        fi
        mkdir -p "$(dirname "$NIXFIED_XTRACE_FILE")" 2>/dev/null || true
        exec 19>>"$NIXFIED_XTRACE_FILE"
        export BASH_XTRACEFD=19
        set -x
      fi

      :
      true
    )
  }

  # 1) default level=info output=stdout preserves prefix/stream semantics.
  C1_OUT="$TMPDIR/case1.out"
  C1_ERR="$TMPDIR/case1.err"
  ( log_info "default-info"; log_warn "default-warn"; log_error "default-error" ) >"$C1_OUT" 2>"$C1_ERR"
  assert_contains "$C1_OUT" "^INFO: default-info$"
  assert_contains "$C1_ERR" "^WARN: default-warn$"
  assert_contains "$C1_ERR" "^ERROR: default-error$"

  # 2) log_info -> stdout.
  C2_OUT="$TMPDIR/case2.out"
  C2_ERR="$TMPDIR/case2.err"
  ( log_info "msg-info" ) >"$C2_OUT" 2>"$C2_ERR"
  assert_contains "$C2_OUT" "^INFO: msg-info$"
  assert_empty "$C2_ERR"

  # 3) log_error -> stderr.
  C3_OUT="$TMPDIR/case3.out"
  C3_ERR="$TMPDIR/case3.err"
  ( log_error "msg-error" ) >"$C3_OUT" 2>"$C3_ERR"
  assert_empty "$C3_OUT"
  assert_contains "$C3_ERR" "^ERROR: msg-error$"

  # 4) debug suppressed at level=info.
  C4_OUT="$TMPDIR/case4.out"
  C4_ERR="$TMPDIR/case4.err"
  ( log_debug "msg-debug-hidden" ) >"$C4_OUT" 2>"$C4_ERR"
  assert_empty "$C4_OUT"
  assert_empty "$C4_ERR"

  # 5) LOG_LEVEL=debug enables debug output.
  C5_OUT="$TMPDIR/case5.out"
  C5_ERR="$TMPDIR/case5.err"
  ( LOG_LEVEL=debug log_debug "msg-debug-visible" ) >"$C5_OUT" 2>"$C5_ERR"
  assert_empty "$C5_OUT"
  assert_contains "$C5_ERR" "^DEBUG: msg-debug-visible$"

  # 6) level=error suppresses info/warn.
  C6_OUT="$TMPDIR/case6.out"
  C6_ERR="$TMPDIR/case6.err"
  (
    LOG_LEVEL=error log_info "msg-info-hidden"
    LOG_LEVEL=error log_warn "msg-warn-hidden"
    LOG_LEVEL=error log_error "msg-error-visible"
  ) >"$C6_OUT" 2>"$C6_ERR"
  assert_empty "$C6_OUT"
  assert_not_contains "$C6_ERR" "msg-warn-hidden"
  assert_contains "$C6_ERR" "^ERROR: msg-error-visible$"

  # 7) Legacy NIXFIED_LOG_LEVEL works when LOG_LEVEL is unset.
  C7_OUT="$TMPDIR/case7.out"
  C7_ERR="$TMPDIR/case7.err"
  (
    unset LOG_LEVEL
    NIXFIED_LOG_LEVEL=warn log_info "msg-info-hidden-2"
    NIXFIED_LOG_LEVEL=warn log_warn "msg-warn-visible"
    NIXFIED_LOG_LEVEL=warn log_debug "msg-debug-hidden-2"
  ) >"$C7_OUT" 2>"$C7_ERR"
  assert_empty "$C7_OUT"
  assert_contains "$C7_ERR" "^WARN: msg-warn-visible$"
  assert_not_contains "$C7_ERR" "msg-info-hidden-2"
  assert_not_contains "$C7_ERR" "msg-debug-hidden-2"

  # 8) OUTPUT_MODE=logs routes non-error logs to file.
  C8_LOG="$TMPDIR/case8.log"
  C8_OUT="$TMPDIR/case8.out"
  C8_ERR="$TMPDIR/case8.err"
  ( OUTPUT_MODE=logs NIXFIED_LOG_FILE="$C8_LOG" log_info "msg-logs-only" ) >"$C8_OUT" 2>"$C8_ERR"
  assert_empty "$C8_OUT"
  assert_empty "$C8_ERR"
  assert_contains "$C8_LOG" "^INFO: msg-logs-only$"

  # 9) Legacy NIXFIED_OUTPUT_MODE still works when OUTPUT_MODE is unset.
  C9_LOG="$TMPDIR/case9.log"
  C9_OUT="$TMPDIR/case9.out"
  C9_ERR="$TMPDIR/case9.err"
  (
    unset OUTPUT_MODE
    NIXFIED_OUTPUT_MODE=both NIXFIED_LOG_FILE="$C9_LOG" log_info "msg-both"
  ) >"$C9_OUT" 2>"$C9_ERR"
  assert_contains "$C9_OUT" "^INFO: msg-both$"
  assert_empty "$C9_ERR"
  assert_contains "$C9_LOG" "^INFO: msg-both$"

  # 10) OUTPUT_MODE=logs still emits ERROR to stderr.
  C10_LOG="$TMPDIR/case10.log"
  C10_OUT="$TMPDIR/case10.out"
  C10_ERR="$TMPDIR/case10.err"
  (
    OUTPUT_MODE=logs NIXFIED_LOG_FILE="$C10_LOG" log_error "msg-logs-error"
  ) >"$C10_OUT" 2>"$C10_ERR"
  assert_empty "$C10_OUT"
  assert_contains "$C10_ERR" "^ERROR: msg-logs-error$"
  assert_contains "$C10_LOG" "^ERROR: msg-logs-error$"

  # 11) level=trace does not enable xtrace without NIXFIED_LOG_TRACE=1.
  C11_TRACE="$TMPDIR/case11.trace.log"
  run_trace_probe "$C11_TRACE" "trace" "0"
  if [ -f "$C11_TRACE" ]; then
    fail "trace file should not exist when NIXFIED_LOG_TRACE is disabled"
  fi

  # 12) level=trace + NIXFIED_LOG_TRACE=1 writes shell xtrace to file.
  C12_TRACE="$TMPDIR/case12.trace.log"
  run_trace_probe "$C12_TRACE" "trace" "1"
  [ -f "$C12_TRACE" ] || fail "trace file missing when trace is enabled"
  assert_contains "$C12_TRACE" "\\+ :"

  echo "helpers logging fixture ok"
''
