{ }:
''
  set -euo pipefail

  fail() {
    echo "FAIL: $*" >&2
    exit 1
  }

  echo "slots runtime start"

  if [ -z "''${SLOT_INFO:-}" ] || [ ! -x "$SLOT_INFO" ]; then
    fail "SLOT_INFO not executable"
  fi

  if [ -z "''${REQUIRE_SLOT_ENV:-}" ] || [ ! -x "$REQUIRE_SLOT_ENV" ]; then
    fail "REQUIRE_SLOT_ENV not executable"
  fi

  load_slot_info() {
    eval "$("$SLOT_INFO")"
  }

  export PROJECT_ENV="dev"
  export NIX_ENV="2"
  load_slot_info
  if [ "$BACKEND_PORT" -ne 3012 ]; then
    fail "dev slot 2 backend port mismatch: $BACKEND_PORT"
  fi
  if [ "$HTTP_PORT" -ne 8092 ]; then
    fail "dev slot 2 http port mismatch: $HTTP_PORT"
  fi

  export PROJECT_ENV="test"
  export NIX_ENV="3"
  load_slot_info
  if [ "$BACKEND_PORT" -ne 3023 ]; then
    fail "test slot 3 backend port mismatch: $BACKEND_PORT"
  fi

  export PROJECT_ENV="prod"
  export NIX_ENV="0"
  load_slot_info
  if [ "$BACKEND_PORT" -ne 3000 ]; then
    fail "prod slot 0 backend port mismatch: $BACKEND_PORT"
  fi

  # SLOT_INFO should fail when env is missing and should not infer from command name.
  unset PROJECT_ENV NIX_ENV NIXFIED_ENV
  export COMMAND_NAME="ci"
  set +e
  OUT=$("$SLOT_INFO" 2>&1 < /dev/null)
  RC=$?
  set -e
  if [ "$RC" -eq 0 ]; then
    fail "expected SLOT_INFO to fail without PROJECT_ENV"
  fi
  echo "$OUT" | grep -q "default slot selected" || fail "missing default slot info"
  echo "$OUT" | grep -q "PROJECT_ENV must be set" || fail "missing explicit env error"
  unset COMMAND_NAME

  # REQUIRE_SLOT_ENV should fail fast when env is missing.
  export NIX_ENV="0"
  unset PROJECT_ENV
  set +e
  OUT=$("$REQUIRE_SLOT_ENV" 2>&1 < /dev/null)
  RC=$?
  set -e
  if [ "$RC" -eq 0 ]; then
    fail "expected REQUIRE_SLOT_ENV to fail when PROJECT_ENV is unset"
  fi
  echo "$OUT" | grep -q "PROJECT_ENV must be set" || fail "missing explicit env error"

  # REQUIRE_SLOT_ENV should default slot when it is missing.
  unset NIX_ENV NIXFIED_ENV
  export PROJECT_ENV="dev"
  set +e
  OUT=$("$REQUIRE_SLOT_ENV" 2>&1 < /dev/null)
  RC=$?
  set -e
  if [ "$RC" -ne 0 ]; then
    fail "expected REQUIRE_SLOT_ENV to default NIX_ENV when unset"
  fi
  echo "$OUT" | grep -q "default slot selected" || fail "missing default slot info"
  echo "$OUT" | grep -q "SLOT=0" || fail "missing default slot output"

  # Compatibility alias: NIXFIED_ENV should be accepted as slot input.
  unset NIX_ENV
  export NIXFIED_ENV="4"
  export PROJECT_ENV="dev"
  load_slot_info
  if [ "$SLOT" != "4" ]; then
    fail "NIXFIED_ENV alias did not set slot (got $SLOT)"
  fi
  if [ "$BACKEND_PORT" -ne 3014 ]; then
    fail "alias slot 4 backend port mismatch: $BACKEND_PORT"
  fi

  # Example: requireSlotEnv rejects invalid slot and env values.
  set +e
  PROJECT_ENV="nope" NIX_ENV="0" "$REQUIRE_SLOT_ENV" >/dev/null 2>&1
  RC=$?
  set -e
  if [ "$RC" -eq 0 ]; then
    fail "expected invalid env to exit non-zero"
  fi

  set +e
  PROJECT_ENV="dev" NIX_ENV="10" "$REQUIRE_SLOT_ENV" >/dev/null 2>&1
  RC=$?
  set -e
  if [ "$RC" -eq 0 ]; then
    fail "expected invalid slot to exit non-zero"
  fi

''
