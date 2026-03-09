{ pkgs }:
let
  shellContract = import ../../nixfied/.framework/lib/shell-contract.nix { inherit pkgs; };
  contract = {
    version = 2;
    name = "contract-smoke";
    allowUnknownArgs = false;
    commandClass = "typed";
    idempotent = true;
    failureCodes = shellContract.defaultFailureCodes;
    args = [
      {
        name = "verbose";
        kind = "flag";
        long = "--verbose";
        short = "-v";
        type = "bool";
      }
      {
        name = "mode";
        kind = "option";
        long = "--mode";
        type = "enum";
        values = [
          "basic"
          "full"
        ];
        required = true;
      }
      {
        name = "target";
        kind = "positional";
        type = "string";
        required = true;
      }
    ];
    env = shellContract.mkRuntimePrimitiveEnvSpecs { } ++ [
      {
        name = "API_URL";
        type = "pathAbs";
        required = true;
        aliases = [ "APP_API_URL" ];
      }
      {
        name = "RETRIES";
        type = "int";
        default = 3;
        min = 1;
        max = 5;
      }
    ];
    outputs = {
      mode = "text";
    };
  };
  runtimePlan = shellContract.mkContractRuntime {
    name = "contract-smoke";
    inherit contract;
  };
in
pkgs.runCommand "shell-contract-runtime-smoke" { } ''
  set -euo pipefail

  fail() {
    echo "ERROR: $*" >&2
    exit 1
  }

  source ${toString shellContract.runtime}
  export NIXFIED_APP_CONTRACT_RUNTIME=${toString runtimePlan}
  export APP_API_URL=/tmp/api

  nixfied_contract_validate_env /tmp/unused
  [ "$API_URL" = "/tmp/api" ] || fail "expected alias-resolved API_URL export"
  [ "$RETRIES" = "3" ] || fail "expected RETRIES default export"
  [ "$LOG_LEVEL" = "info" ] || fail "expected LOG_LEVEL runtime default"
  [ "$OUTPUT_MODE" = "stdout" ] || fail "expected OUTPUT_MODE runtime default"

  nixfied_contract_validate_args /tmp/unused -v --mode full target-name
  [ "$NIXFIED_ARG_VERBOSE" = "true" ] || fail "expected verbose flag export"
  [ "$NIXFIED_ARG_MODE" = "full" ] || fail "expected mode option export"
  [ "$NIXFIED_ARG_TARGET" = "target-name" ] || fail "expected target positional export"

  nixfied_contract_validate_exit /tmp/unused 2

  set +e
  env -i \
    PATH="$PATH" \
    HOME="$HOME" \
    TMPDIR="$TMPDIR" \
    NIXFIED_APP_CONTRACT_RUNTIME=${toString runtimePlan} \
    APP_API_URL=/tmp/api \
    NIXFIED_LOG_LEVEL=debug \
    NIXFIED_OUTPUT_MODE=logs \
    ${pkgs.bash}/bin/bash -lc '
      set -euo pipefail
      source ${toString shellContract.runtime}
      nixfied_contract_validate_env /tmp/unused
      [ "$LOG_LEVEL" = "debug" ]
      [ "$NIXFIED_LOG_LEVEL" = "debug" ]
      [ "$OUTPUT_MODE" = "logs" ]
      [ "$NIXFIED_OUTPUT_MODE" = "logs" ]
    ' > "$TMPDIR/runtime-alias.out" 2>&1
  runtime_alias_rc="$?"
  set -e
  if [ "$runtime_alias_rc" -ne 0 ]; then
    fail "expected runtime aliases to resolve through compiled contract plan"
  fi

  set +e
  nixfied_contract_validate_args /tmp/unused --mode invalid target-name > "$TMPDIR/invalid-arg.out" 2>&1
  invalid_arg_rc="$?"
  set -e
  if [ "$invalid_arg_rc" -eq 0 ]; then
    fail "expected invalid enum arg to fail"
  fi
  ${pkgs.gnugrep}/bin/grep -F "ERROR: arg:mode must be one of basic,full (got 'invalid')" "$TMPDIR/invalid-arg.out" >/dev/null \
    || fail "expected invalid enum arg message"

  set +e
  nixfied_contract_validate_exit /tmp/unused 99 > "$TMPDIR/invalid-exit.out" 2>&1
  invalid_exit_rc="$?"
  set -e
  if [ "$invalid_exit_rc" -eq 0 ]; then
    fail "expected undeclared exit code to fail"
  fi
  ${pkgs.gnugrep}/bin/grep -F "ERROR: undeclared exit code code=99" "$TMPDIR/invalid-exit.out" >/dev/null \
    || fail "expected undeclared exit code message"

  echo "OK: compiled shell contract plan validates env, args, and exits" > "$out"
''
