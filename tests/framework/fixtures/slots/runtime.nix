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

  # Example: resolveEnv uses COMMAND_NAME when PROJECT_ENV is unset.
  unset PROJECT_ENV NIX_ENV
  export COMMAND_NAME="ci"
  ENV_OUT=$("$SLOT_INFO" | grep "^ENV=" | cut -d= -f2)
  if [ "$ENV_OUT" != "test" ]; then
    fail "resolveEnv did not infer test from COMMAND_NAME=ci (got $ENV_OUT)"
  fi
  unset COMMAND_NAME

  export COMMAND_NAME="dev"
  ENV_OUT=$("$SLOT_INFO" | grep "^ENV=" | cut -d= -f2)
  if [ "$ENV_OUT" != "dev" ]; then
    fail "resolveEnv did not infer dev from COMMAND_NAME=dev (got $ENV_OUT)"
  fi
  unset COMMAND_NAME

  export COMMAND_NAME="prod"
  ENV_OUT=$("$SLOT_INFO" | grep "^ENV=" | cut -d= -f2)
  if [ "$ENV_OUT" != "prod" ]; then
    fail "resolveEnv did not infer prod from COMMAND_NAME=prod (got $ENV_OUT)"
  fi
  unset COMMAND_NAME

  export TERM=dumb
  export NO_TTY=1
  export CI=1
  set +e
  OUT=$("$REQUIRE_SLOT_ENV" 2>&1 < /dev/null)
  RC=$?
  set -e
  if [ "$RC" -ne 0 ]; then
    echo "REQUIRE_SLOT_ENV non-tty failed (rc=$RC)" >&2
    echo "$OUT" >&2
    fail "require_slot_env non-tty failure"
  fi
  echo "$OUT" | grep -q "Continue with these defaults" && fail "non-tty should not prompt"
  echo "$OUT" | grep -q "SLOT=0" || fail "missing SLOT output"
  echo "$OUT" | grep -q "ENV=dev" || fail "missing default env output"

  unset CI NO_TTY
  export TERM=xterm-256color

  python3 - <<PY
  import os
  import pty
  import select
  import subprocess
  import sys
  import time

  cmd = [os.environ["REQUIRE_SLOT_ENV"]]
  env = os.environ.copy()
  env.pop("PROJECT_ENV", None)
  env.pop("NIX_ENV", None)
  env.pop("CI", None)
  env.pop("NO_TTY", None)
  env["TERM"] = "xterm-256color"

  master, slave = pty.openpty()
  proc = subprocess.Popen(
      cmd,
      env=env,
      stdin=slave,
      stdout=slave,
      stderr=slave,
      close_fds=True,
  )
  os.close(slave)
  os.write(master, b"n\n")

  output = b""
  deadline = time.time() + 5
  while True:
      if time.time() > deadline:
          proc.kill()
          print("timeout waiting for prompt")
          sys.exit(1)
      r, _, _ = select.select([master], [], [], 0.2)
      if r:
          try:
              chunk = os.read(master, 1024)
          except OSError:
              break
          if not chunk:
              break
          output += chunk
      if proc.poll() is not None and not r:
          break

  rc = proc.wait()
  try:
      os.close(master)
  except OSError:
      pass

  text = output.decode("utf-8", "ignore")
  if rc == 0:
      print("expected non-zero rc")
      print(text)
      sys.exit(1)
  if "Continue with these defaults" not in text:
      print("missing prompt text")
      print(text)
      sys.exit(1)
  if "Aborted." not in text:
      print("missing abort message")
      print(text)
      sys.exit(1)
  PY

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
