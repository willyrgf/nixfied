{ pkgs }:
let
  serviceApi = import ../../nixfied/framework/core/service-api.nix { inherit pkgs; };
  serviceContractValidation = import ../../nixfied/framework/core/service-contract-validation.nix {
    inherit pkgs;
  };
  runtimePrimitives = import ../../nixfied/framework/core/runtime-primitives.nix { };
  shellHelpers = import ./lib/shell-helpers.nix { inherit pkgs; };
  serviceRuntimePrimitives = runtimePrimitives.mkServiceRuntimePrimitivesV1 { };

  prepareScript = pkgs.writeShellScript "service-op-prepare" ''
    set -euo pipefail
    printf '%s\n' "prepare" >> "$COMPOSITION_LOG"
  '';

  mainScript = pkgs.writeShellScript "service-op-main" ''
    set -euo pipefail
    printf 'main:%s\n' "$*" >> "$COMPOSITION_LOG"
  '';

  finalizeScript = pkgs.writeShellScript "service-op-finalize" ''
    set -euo pipefail
    printf '%s\n' "finalize" >> "$COMPOSITION_LOG"
  '';

  stopScript = pkgs.writeShellScript "service-op-stop" ''
    set -euo pipefail
    printf '%s\n' "stop" >> "$COMPOSITION_LOG"
  '';

  statusScript = pkgs.writeShellScript "service-op-status" ''
    set -euo pipefail
    exit 0
  '';

  mkContract = operations: {
    version = 1;
    service = "demo";
    summary = "demo";
    details = "demo service contract";
    ownerFile = "tests/framework/service-op-composition-contract.nix";
    artifacts = { };
    inherit operations;
    runtimePrimitives = serviceRuntimePrimitives;
  };

  demoContract = mkContract {
    prepare = {
      runtimeOp = "prepare";
      summary = "prepare";
      details = "prepare";
      exposeApp = false;
      exposeHook = false;
    };
    start = {
      runtimeOp = "start";
      preOps = [ "prepare" ];
      postOps = [ "finalize" ];
      summary = "start";
      details = "start";
    };
    finalize = {
      runtimeOp = "finalize";
      summary = "finalize";
      details = "finalize";
      exposeApp = false;
      exposeHook = false;
    };
    stop = {
      runtimeOp = "stop";
      summary = "stop";
      details = "stop";
    };
    status = {
      runtimeOp = "status";
      summary = "status";
      details = "status";
    };
    restart = {
      runtimeOp = null;
      preOps = [
        "stop"
        "start"
      ];
      summary = "restart";
      details = "restart";
    };
  };

  demoImplementation = {
    version = 1;
    operations = {
      prepare = prepareScript;
      start = mainScript;
      finalize = finalizeScript;
      stop = stopScript;
      status = statusScript;
    };
  };

  mkDemoOperationMetadata =
    opName: opCfg:
    let
      appName =
        if (opCfg.appName or null) != null && opCfg.appName != "" then
          opCfg.appName
        else
          "svc::demo::${opName}";
    in
    {
      inherit appName;
      hookName = "SVC_DEMO_${pkgs.lib.toUpper opName}";
      includeApp = opCfg.exposeApp or true;
      includeHook = opCfg.exposeHook or true;
      usage = opCfg.usage or [ "nix run .#${appName}" ];
      category = if (opCfg.category or "") != "" then opCfg.category else "demo";
      class = opCfg.class or "passthrough";
      idempotent = opCfg.idempotent or false;
      summary = opCfg.summary;
      details = opCfg.details;
      examples = opCfg.examples or [ ];
      args = opCfg.args or [ ];
      env = opCfg.env or [ ];
    };

  demoOperationCatalog = {
    demo = builtins.mapAttrs mkDemoOperationMetadata demoContract.operations;
  };

  demoOps = serviceApi.collectServiceOpsFromCatalog {
    serviceContracts = {
      demo = demoContract;
    };
    operationCatalog = demoOperationCatalog;
    serviceImplementations = {
      demo = demoImplementation;
    };
  };

  findOp = opName: builtins.head (builtins.filter (op: op.opName == opName) demoOps);

  startOp = findOp "start";
  restartOp = findOp "restart";
  hookEnv = serviceApi.mkServiceHookEnvFromCatalog {
    serviceContracts = {
      demo = demoContract;
    };
    operationCatalog = demoOperationCatalog;
    serviceImplementations = {
      demo = demoImplementation;
    };
  };

  unknownRefContract = mkContract {
    start = {
      runtimeOp = "start";
      preOps = [ "missing" ];
      summary = "start";
      details = "start";
    };
    stop = {
      runtimeOp = "stop";
      summary = "stop";
      details = "stop";
    };
    status = {
      runtimeOp = "status";
      summary = "status";
      details = "status";
    };
  };

  cycleContract = mkContract {
    start = {
      runtimeOp = "start";
      preOps = [ "restart" ];
      summary = "start";
      details = "start";
    };
    stop = {
      runtimeOp = "stop";
      summary = "stop";
      details = "stop";
    };
    status = {
      runtimeOp = "status";
      summary = "status";
      details = "status";
    };
    restart = {
      runtimeOp = null;
      preOps = [ "start" ];
      summary = "restart";
      details = "restart";
    };
  };

  unknownRefResult = builtins.tryEval (
    builtins.deepSeq (serviceContractValidation.validateServiceContracts {
      demo = unknownRefContract;
    }) true
  );

  cycleResult = builtins.tryEval (
    builtins.deepSeq (serviceContractValidation.validateServiceContracts { demo = cycleContract; }) true
  );
in
assert builtins.hasAttr "SVC_DEMO_START" hookEnv;
assert builtins.hasAttr "SVC_DEMO_RESTART" hookEnv;
assert builtins.hasAttr "SVC_DEMO_STATUS" hookEnv;
assert !(builtins.hasAttr "SVC_DEMO_PREPARE" hookEnv);
assert !(builtins.hasAttr "SVC_DEMO_FINALIZE" hookEnv);
assert unknownRefResult.success == false;
assert cycleResult.success == false;
pkgs.runCommand "service-op-composition-contract" { } ''
  set -euo pipefail
  ${shellHelpers.shellPrelude}

  export COMPOSITION_LOG="$TMPDIR/composition.log"
  : > "$COMPOSITION_LOG"

  "${startOp.launcher}" alpha beta > "$TMPDIR/start.out" 2>&1 || {
    cat "$TMPDIR/start.out" >&2
    fail "start launcher should succeed"
  }

  expected_start="$(printf 'prepare\nmain:alpha beta\nfinalize\n')"
  actual_start="$(cat "$COMPOSITION_LOG")"
  if [ "$actual_start" != "$expected_start" ]; then
    printf 'expected:\n%s\nactual:\n%s\n' "$expected_start" "$actual_start" >&2
    fail "start launcher should execute pre, main, and post ops in order"
  fi

  : > "$COMPOSITION_LOG"

  "${restartOp.launcher}" ignored > "$TMPDIR/restart.out" 2>&1 || {
    cat "$TMPDIR/restart.out" >&2
    fail "restart launcher should succeed"
  }

  expected_restart="$(printf 'stop\nprepare\nmain:\nfinalize\n')"
  actual_restart="$(cat "$COMPOSITION_LOG")"
  if [ "$actual_restart" != "$expected_restart" ]; then
    printf 'expected:\n%s\nactual:\n%s\n' "$expected_restart" "$actual_restart" >&2
    fail "restart launcher should flatten nested preOps without forwarding root args"
  fi

  echo "OK: service op composition validates refs/cycles, preserves ordering, scopes args, and filters hidden hooks" > "$out"
''
