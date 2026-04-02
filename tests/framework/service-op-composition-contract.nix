{ pkgs }:
let
  mkServiceRuntimeSurfaces = import ../../nixfied/framework/core/mkServiceRuntimeSurfaces.nix;
  commandApi = import ../../nixfied/framework/core/command-api.nix { inherit pkgs; };
  serviceContractValidation = import ../../nixfied/framework/core/service-contract-validation.nix {
    inherit pkgs;
  };
  runtimePrimitives = import ../../nixfied/framework/core/runtime-primitives.nix { };
  shellHelpers = import ./lib/shell-helpers.nix { inherit pkgs; };
  serviceRuntimePrimitives = runtimePrimitives.mkServiceRuntimePrimitivesV1 { };
  inherit (commandApi) mkCommandApi;

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

  demoImplementationModule = pkgs.writeText "demo-service-implementation.nix" ''
    { pkgs, slots, project }:
    {
      version = 1;
      operations = {
        prepare = ${prepareScript};
        start = ${mainScript};
        finalize = ${finalizeScript};
        stop = ${stopScript};
        status = ${statusScript};
      };
    }
  '';

  mkDemoOperationMetadata =
    opName: opCfg:
    let
      appName =
        if (opCfg.appName or null) != null && opCfg.appName != "" then
          opCfg.appName
        else
          "svc::demo::${opName}";
      category = if (opCfg.category or "") != "" then opCfg.category else "demo";
    in
    {
      inherit appName;
      hookName = "SVC_DEMO_${pkgs.lib.toUpper opName}";
      includeApp = opCfg.exposeApp or true;
      includeHook = opCfg.exposeHook or true;
      usage = opCfg.usage or [ "nix run .#${appName}" ];
      inherit category;
      class = opCfg.class or "passthrough";
      idempotent = opCfg.idempotent or false;
      summary = opCfg.summary;
      details = opCfg.details;
      examples = opCfg.examples or [ ];
      args = opCfg.args or [ ];
      env = opCfg.env or [ ];
      commandApi =
        (mkCommandApi {
          class = opCfg.class or "passthrough";
          name = appName;
          summary = opCfg.summary;
          details = opCfg.details or "";
          usage = opCfg.usage or [ "nix run .#${appName}" ];
          examples = opCfg.examples or [ ];
          args = opCfg.args or [ ];
          env = opCfg.env or [ ];
          inherit category;
          idempotent = opCfg.idempotent or false;
        }).commandApi;
    };

  demoOperationCatalog = {
    demo = builtins.mapAttrs mkDemoOperationMetadata demoContract.operations;
  };

  demoRuntimeSurfaces = mkServiceRuntimeSurfaces {
    inherit pkgs;
    model = {
      identity = {
        projectId = "demo";
      };
      runtime = {
        ports = { };
        env = {
          offsets = {
            dev = 0;
          };
          var = "PROJECT_ENV";
          default = "dev";
          names = [ "dev" ];
        };
        slot = {
          var = "NIX_ENV";
          default = 0;
          stride = 100;
          max = 8;
        };
        directories = {
          base = "/tmp/nixfied-demo";
        };
        logging = {
          levelDefault = "info";
          outputDefault = "stdout";
        };
      };
      state = {
        policy = {
          artifactsRoot = "/tmp/nixfied-demo/artifacts";
        };
      };
    };
    services = {
      demo = {
        enable = true;
        name = "demo";
        config = {
          dataDirName = "demo";
        };
      };
    };
    serviceSurfaceCatalog = {
      serviceApis = {
        demo = demoContract;
      };
      operationCatalog = demoOperationCatalog;
    };
    serviceDefinitions = {
      demo = {
        implementation = {
          module = demoImplementationModule;
        };
      };
    };
  };
  hookEnv = demoRuntimeSurfaces.serviceHookEnv;
  startProgram = hookEnv.SVC_DEMO_START;
  restartProgram = hookEnv.SVC_DEMO_RESTART;

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
  export LOG_LEVEL=info
  export OUTPUT_MODE=stdout
  : > "$COMPOSITION_LOG"

  "${startProgram}" alpha beta > "$TMPDIR/start.out" 2>&1 || {
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

  "${restartProgram}" ignored > "$TMPDIR/restart.out" 2>&1 || {
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
