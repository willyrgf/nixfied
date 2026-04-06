{ pkgs }:
let
  frameworkLib = import ../../nixfied/framework/core {
    inherit pkgs;
    system = pkgs.system;
  };
  shellHelpers = import ./lib/shell-helpers.nix { inherit pkgs; };

  prepareScript = pkgs.writeShellScript "demo-service-prepare" ''
    set -euo pipefail
    printf '%s\n' "prepare" >> "$COMPOSITION_LOG"
  '';

  mainScript = pkgs.writeShellScript "demo-service-start" ''
    set -euo pipefail
    printf 'main:%s\n' "$*" >> "$COMPOSITION_LOG"
  '';

  finalizeScript = pkgs.writeShellScript "demo-service-finalize" ''
    set -euo pipefail
    printf '%s\n' "finalize" >> "$COMPOSITION_LOG"
  '';

  stopScript = pkgs.writeShellScript "demo-service-stop" ''
    set -euo pipefail
    printf '%s\n' "stop" >> "$COMPOSITION_LOG"
  '';

  noopScript = pkgs.writeShellScript "demo-service-noop" ''
    set -euo pipefail
    exit 0
  '';

  statusScript = pkgs.writeShellScript "demo-service-status" ''
    set -euo pipefail
    exit 0
  '';

  demoImplementationModule = pkgs.writeText "demo-service-implementation.nix" ''
    { pkgs, slots, project }:
    {
      version = 1;
      operations = {
        "pre-start" = ${noopScript};
        start = ${mainScript};
        status = ${statusScript};
        "pre-stop" = ${noopScript};
        stop = ${stopScript};
        health = ${statusScript};
        ready = ${statusScript};
        prepare = ${prepareScript};
        finalize = ${finalizeScript};
      };
    }
  '';

  demoExtraOps = {
    prepare = {
      runtimeOp = "prepare";
      summary = "Prepare demo startup";
      details = "Runs pre-start composition for the demo service.";
      exposeApp = false;
    };

    finalize = {
      runtimeOp = "finalize";
      summary = "Finalize demo startup";
      details = "Runs post-start composition for the demo service.";
      exposeApp = false;
    };

    restart = {
      runtimeOp = null;
      preOps = [
        "stop"
        "start"
      ];
      summary = "Restart demo";
      details = "Stops then starts the demo service.";
    };
  };

  mkDemoModule = extraOps: {
    nixfied.services.demo = {
      enable = true;
      displayName = "Demo";
      summary = "Demo service management API";
      details = "Public service contract for demo service composition checks.";
      ownerFile = "tests/framework/service-op-composition-contract.nix";

      lifecycle = {
        preStart = {
          summary = "Prepare Demo startup";
          details = "Runs deterministic pre-start steps for the demo service.";
        };
        start = {
          summary = "Start Demo";
          details = "Starts the demo service.";
          preOps = [ "prepare" ];
          postOps = [ "finalize" ];
        };
        status = {
          summary = "Show Demo status";
          details = "Prints demo service status.";
        };
        preStop = {
          summary = "Prepare Demo shutdown";
          details = "Runs deterministic pre-stop steps for the demo service.";
        };
        stop = {
          summary = "Stop Demo";
          details = "Stops the demo service.";
        };
      };

      checks = {
        health = {
          summary = "Run Demo health check";
          details = "Checks demo service health.";
        };
        ready = {
          summary = "Wait for Demo readiness";
          details = "Waits for demo service readiness.";
        };
      };

      extraOps = extraOps;

      implementation = {
        version = 1;
        module = demoImplementationModule;
      };
    };
  };

  frameworkOutputs = frameworkLib.mkFlakeOutputs {
    projectRoot = ../..;
    projectModules = [ ../../nixfied/project/module.nix ];
    extraModules = [ (mkDemoModule demoExtraOps) ];
    localOverrides = [ ];
  };

  startProgram = frameworkOutputs.apps."svc::demo::start".program;
  restartProgram = frameworkOutputs.apps."svc::demo::restart".program;

  unknownRefResult = builtins.tryEval (
    builtins.deepSeq ((frameworkLib.mkNixfied {
      projectRoot = ../..;
      projectModules = [ ../../nixfied/project/module.nix ];
      extraModules = [
        (mkDemoModule (
          demoExtraOps
          // {
            restart = demoExtraOps.restart // {
              preOps = [ "missing" ];
            };
          }
        ))
      ];
      localOverrides = [ ];
    }).model.compiled.serviceSurfaceCatalog
    ) true
  );

  cycleResult = builtins.tryEval (
    builtins.deepSeq ((frameworkLib.mkNixfied {
      projectRoot = ../..;
      projectModules = [ ../../nixfied/project/module.nix ];
      extraModules = [
        (mkDemoModule (
          demoExtraOps
          // {
            prepare = demoExtraOps.prepare // {
              preOps = [ "restart" ];
            };
            restart = demoExtraOps.restart // {
              preOps = [ "prepare" ];
            };
          }
        ))
      ];
      localOverrides = [ ];
    }).model.compiled.serviceSurfaceCatalog
    ) true
  );
in
assert builtins.hasAttr "svc::demo::start" frameworkOutputs.apps;
assert builtins.hasAttr "svc::demo::restart" frameworkOutputs.apps;
assert builtins.hasAttr "svc::demo::status" frameworkOutputs.apps;
assert !(builtins.hasAttr "svc::demo::prepare" frameworkOutputs.apps);
assert !(builtins.hasAttr "svc::demo::finalize" frameworkOutputs.apps);
assert unknownRefResult.success == false;
assert cycleResult.success == false;
pkgs.runCommand "service-op-composition-contract" { } ''
  set -euo pipefail
  ${shellHelpers.shellPrelude}

  export COMPOSITION_LOG="$TMPDIR/composition.log"
  export LOG_LEVEL=info
  export OUTPUT_MODE=stdout
  export REGISTRY_ROOT="$TMPDIR/registry"
  export CI_ARTIFACTS_ROOT="$TMPDIR/ci-artifacts"
  export NIXFIED_RUNTIME_DIR_SCOPE_OVERRIDE="$TMPDIR/runtime-scope"
  mkdir -p "$REGISTRY_ROOT" "$CI_ARTIFACTS_ROOT" "$NIXFIED_RUNTIME_DIR_SCOPE_OVERRIDE"
  : > "$COMPOSITION_LOG"

  "${startProgram}" alpha beta > "$TMPDIR/start.out" 2>&1 || {
    cat "$TMPDIR/start.out" >&2
    fail "public svc::demo::start app should succeed"
  }

  expected_start="$(printf 'prepare\nmain:alpha beta\nfinalize\n')"
  actual_start="$(cat "$COMPOSITION_LOG")"
  if [ "$actual_start" != "$expected_start" ]; then
    printf 'expected:\n%s\nactual:\n%s\n' "$expected_start" "$actual_start" >&2
    fail "start app should execute pre, main, and post ops in order"
  fi

  : > "$COMPOSITION_LOG"

  "${restartProgram}" ignored > "$TMPDIR/restart.out" 2>&1 || {
    cat "$TMPDIR/restart.out" >&2
    fail "public svc::demo::restart app should succeed"
  }

  expected_restart="$(printf 'stop\nprepare\nmain:\nfinalize\n')"
  actual_restart="$(cat "$COMPOSITION_LOG")"
  if [ "$actual_restart" != "$expected_restart" ]; then
    printf 'expected:\n%s\nactual:\n%s\n' "$expected_restart" "$actual_restart" >&2
    fail "restart app should flatten nested preOps without forwarding root args"
  fi

  echo "OK: public svc apps preserve service op composition, hidden ops stay hidden, and invalid refs still fail validation" > "$out"
''
