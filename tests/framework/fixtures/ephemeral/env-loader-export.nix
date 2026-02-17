{ }:
''
  fail() {
    echo "FAIL: $*" >&2
    exit 1
  }

  EXPECTED="''${EXPECTED_HELIOS_NETWORK:-}"
  ACTUAL="''${HELIOS_NETWORK:-}"

  if [ -z "$EXPECTED" ]; then
    fail "EXPECTED_HELIOS_NETWORK must be set"
  fi

  if [ "$ACTUAL" != "$EXPECTED" ]; then
    fail "HELIOS_NETWORK mismatch expected=$EXPECTED actual=''${ACTUAL:-<unset>}"
  fi

  echo "OK: ephemeral wrapper env propagation expected=$EXPECTED actual=$ACTUAL"
''
