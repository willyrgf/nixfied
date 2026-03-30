{ pkgs }:
let
  slotEnvRuntime = import ../../nixfied/framework/core/slot-env-runtime.nix {
    inherit pkgs;
  };
in
pkgs.runCommand "slot-env-runtime-smoke" { } ''
    set -euo pipefail

    fail() {
      echo "ERROR: $*" >&2
      exit 1
    }

    SLOT_INFO_JSON="$(
      cat <<'EOF'
  SLOT='7'
  ENV='prod'
  FOO='bar'
  COUNT='3'
  RUN_DIR='/tmp/runtime-run'
  HTTP_PORT='8080'
  EOF
    )"

    ${slotEnvRuntime.loadSlotEnvAndVarsFromJson {
      jsonVar = "SLOT_INFO_JSON";
      slotVar = "SLOT_ID";
      envVar = "ENV_ID";
    }}

    [ "$SLOT_ID" = "7" ] || fail "expected slot export"
    [ "$ENV_ID" = "prod" ] || fail "expected env export"
    [ "$FOO" = "bar" ] || fail "expected vars export"
    [ "$COUNT" = "3" ] || fail "expected numeric vars coerced to strings"

    ${slotEnvRuntime.readJsonField {
      targetVar = "RUN_DIR";
      jsonVar = "SLOT_INFO_JSON";
      fieldExpr = ".directories.run";
    }}
    [ "$RUN_DIR" = "/tmp/runtime-run" ] || fail "expected directory field"

    ${slotEnvRuntime.readPortFromJson {
      targetVar = "HTTP_PORT";
      jsonVar = "SLOT_INFO_JSON";
      keyExpr = "HTTP_PORT";
    }}
    [ "$HTTP_PORT" = "8080" ] || fail "expected port field"

    echo "OK: slot env runtime loads slot/env/vars from one JSON payload" > "$out"
''
