# Ephemeral slot locking test fixture
{
  acquireSlotLock,
  releaseSlotLock,
  projectIdUpper,
}:
let
  slotVarName = "${projectIdUpper}_EPHEMERAL_SLOT";
  fdVarName = "${projectIdUpper}_SLOT_LOCK_FD";
in
''
  fail() {
    echo "FAIL: $*" >&2
    exit 1
  }

  # Test 1: acquire a slot lock
  LOCK_OUTPUT=$(${acquireSlotLock})
  if [ $? -ne 0 ]; then
    fail "acquireSlotLock exited non-zero"
  fi

  # The output should contain export statements
  echo "$LOCK_OUTPUT" | grep -q "export ${slotVarName}=" || \
    fail "expected ${slotVarName} export in output"
  echo "$LOCK_OUTPUT" | grep -q "export ${fdVarName}=" || \
    fail "expected ${fdVarName} export in output"

  # Eval the output to set vars
  eval "$LOCK_OUTPUT"

  # Use bash indirect expansion to read the dynamically-named vars
  _SLOT_VAR="${slotVarName}"
  _FD_VAR="${fdVarName}"
  SLOT_VAL="''${!_SLOT_VAR:-}"
  FD_VAL="''${!_FD_VAR:-}"

  # Test 2: slot should be a number 0-9
  if ! echo "$SLOT_VAL" | grep -qE '^[0-9]$'; then
    fail "slot should be 0-9, got: $SLOT_VAL"
  fi

  # Test 3: lock FD should be set
  if [ -z "$FD_VAL" ]; then
    fail "lock FD should be set"
  fi

  # Test 4: lock file should exist
  LOCK_FILE="/tmp/nixfied-project-slot-$SLOT_VAL.lock"
  if [ ! -f "$LOCK_FILE" ]; then
    fail "expected lock file: $LOCK_FILE"
  fi

  # Test 5: release the lock
  ${releaseSlotLock}

  echo "ephemeral lock tests passed"
''
