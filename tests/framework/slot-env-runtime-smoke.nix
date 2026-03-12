{ pkgs }:
let
  slotEnvRuntime = import ../../nixfied/framework/runtime/helpers/slot-env-runtime.nix {
    inherit pkgs;
  };
in
pkgs.runCommand "slot-env-runtime-smoke" { } ''
  set -euo pipefail

  fail() {
    echo "ERROR: $*" >&2
    exit 1
  }

  SLOT_INFO_JSON='{"slot":"7","env":"prod","vars":{"FOO":"bar","COUNT":3},"ports":{"HTTP_PORT":8080},"directories":{"run":"/tmp/runtime-run"}}'

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
    jqExpr = ".directories.run";
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
