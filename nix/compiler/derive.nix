{
  lib,
  pkgs,
  system,
  constants,
  config,
}:

let
  targetLib = import ../lib/target.nix { inherit lib; };
  identifiers = import ../lib/identifiers.nix;
  surfaceNames = [
    "model"
    "schema"
    "docs"
    "capabilities"
    "check"
    "run"
    "ps"
    "down"
    "clean"
  ];
  surfaceSpec = name: {
    inherit name;
    aliases = [ ];
    inputSchema = { };
    outputSchema = { };
    exitClasses = [
      "ok"
      "error"
    ];
    evaluationPermission = "never";
    maturity = "m0";
  };
  target = targetLib.fromSystem config.nixfied.target.system;
  closure = import ../lib/closures.nix {
    inherit pkgs target;
  };
  portWindow = config.nixfied.services.synthetic.portWindow;
  serviceIdentityInputs = {
    projectId = config.nixfied.project.projectId;
    environment = "dev";
    slot = 0;
    service = "synthetic";
    endpoint = {
      protocol = "tcp";
      host = "127.0.0.1";
      inherit (portWindow) start end;
    };
    state = {
      inherit (config.nixfied.state) stateEpoch cleanupPolicy persistence;
    };
    runtime = {
      exec = closure.spec.executable;
        lifecycle = closure.spec.operationBindings;
      operationArgs = {
          start = [
            "service"
            "--host"
            "127.0.0.1"
            "--port"
            "\${port}"
          ];
          stop = [ "stop" ];
          task = [
            "task"
            "--host"
            "127.0.0.1"
            "--port"
            "\${port}"
          ];
      };
      readiness = {
        probe = {
          kind = "tcp-connect";
          endpointId = "synthetic-tcp";
        };
        inherit (config.nixfied.services.synthetic.readiness)
          timeoutMs
          retryIntervalMs
          maxAttempts
          ;
      };
      stopPolicy = {
        signal = "TERM";
        timeoutMs = config.nixfied.services.synthetic.stopTimeoutMs;
      };
      containment = "process-group";
    };
    inherit target;
  };
in
{
  inherit (closure) package;

  model = {
    modelVersion = constants.modelVersion;
    toolchainId = constants.toolchainId;
    runtimeAbi = constants.runtimeAbi;
    generator = {
      name = "nixfied";
      version = constants.toolchainId;
      emitter = "nix/compiler/emit-model.nix";
    };
    project = {
      inherit (config.nixfied.project) projectId name;
    };
    inherit target;
    codebases = [
      {
        codebaseId = "main";
        inherit (config.nixfied.codebases.main) logicalRoot sourceIdentity;
        sourceMode = "live-workspace";
        sourcePolicy = {
          inherit (config.nixfied.codebases.main) dirtyPolicy admissionFingerprintPolicy;
        };
      }
    ];
    environments = {
      dev = {
        environmentId = "dev";
        inherit (config.nixfied.environments.dev) services tasks;
      };
    };
    inherit (config.nixfied) slotPolicy;
    capabilities = {
      environments = [ "dev" ];
      slots = [ 0 ];
      services = [ "synthetic" ];
      tasks = [ "smoke" ];
      workflows = [ ];
      surfaces = surfaceNames;
    };
    runtimeConstraints = {
      allowedEnvironments = [ "dev" ];
      slotMin = 0;
      slotDefault = 0;
      slotMax = 0;
      allowPortOverride = false;
      collisionPolicy = "fail";
    };
    surfaces = map surfaceSpec surfaceNames;
    placement = {
      stateRootTemplate = "\${projectId}/\${environment}/\${slot}";
      registryDir = "registry";
      runDirTemplate = "runs/\${runId}";
      logsDirTemplate = "runs/\${runId}/logs";
      artifactsDirTemplate = "runs/\${runId}/artifacts";
      candidatePorts = {
        inherit (portWindow) start end;
      };
    };
    state = {
      inherit (config.nixfied.state)
        markerIdentity
        stateEpoch
        cleanupPolicy
        persistence
        ;
    };
    secrets = [ ];
    closures = [ closure.spec ];
    execs = {
      m0-helper = {
        execId = "m0-helper";
        closureId = "m0-helper";
        executable = closure.spec.executable;
        args = [ ];
        env = { };
        codebaseId = "main";
        cwd = ".";
        stdin = "null";
        timeoutMs = config.nixfied.tasks.smoke.timeoutMs;
        outputCapture = "stdout-stderr";
        cancellationMode = "kill-process-group";
      };
    };
    services = {
      synthetic = {
        serviceId = "synthetic";
        foreground = true;
        lifecycle = [
          {
            operationId = "service.synthetic.start";
            class = "start";
            execId = "m0-helper";
            execArgs = serviceIdentityInputs.runtime.operationArgs.start;
            probeId = null;
            terminal = {
              success = "spawned";
              failure = "failed";
            };
          }
          {
            operationId = "service.synthetic.ready";
            class = "ready";
            execId = null;
            execArgs = [ ];
            probeId = "synthetic-tcp";
            terminal = {
              success = "ready";
              failure = "not-ready";
            };
          }
          {
            operationId = "service.synthetic.stop";
            class = "stop";
            execId = "m0-helper";
            execArgs = serviceIdentityInputs.runtime.operationArgs.stop;
            probeId = null;
            terminal = {
              success = "stopped";
              failure = "failed";
            };
          }
        ];
        endpoints = [
          {
            endpointId = "synthetic-tcp";
            protocol = "tcp";
            host = "127.0.0.1";
            port = {
              kind = "candidate-window";
              inherit (portWindow) start end;
            };
            ownershipVerification = "required";
            socketActivation = "disabled";
          }
        ];
        probes = [
          {
            probeId = "synthetic-tcp";
            target = {
              kind = "tcp-connect";
              endpointId = "synthetic-tcp";
            };
            inherit (config.nixfied.services.synthetic.readiness)
              timeoutMs
              retryIntervalMs
              maxAttempts
              ;
          }
        ];
        readinessProbe = "synthetic-tcp";
        stopPolicy = {
          inherit (serviceIdentityInputs.runtime.stopPolicy) signal timeoutMs;
        };
        stateRefs = [ "slot" ];
        logRefs = [ "service.synthetic" ];
        containment = "process-group";
        lifetime = "run-scoped";
        identity = {
          serviceAddressHash = identifiers.hashJson {
            inherit (serviceIdentityInputs) projectId environment slot service;
          };
          endpointIdentityHash = identifiers.hashJson serviceIdentityInputs.endpoint;
          stateIdentityHash = identifiers.hashJson serviceIdentityInputs.state;
          runtimeCompatibilityHash = identifiers.hashJson serviceIdentityInputs.runtime;
          targetIdentityHash = identifiers.hashJson serviceIdentityInputs.target;
        };
      };
    };
    tasks = {
      smoke = {
        taskId = "smoke";
        operationId = "task.smoke.run";
        execId = "m0-helper";
        args = serviceIdentityInputs.runtime.operationArgs.task;
        dependsOnServicesReady = [ "synthetic" ];
        exitPolicy = {
          successCodes = [ 0 ];
        };
        outputCapture = "stdout-stderr";
        artifactRefs = [ ];
        logRefs = [ "task.smoke" ];
        summaryRefs = [ "summary" ];
      };
    };
    workflows = { };
    docs = {
      title = config.nixfied.project.name;
      summary = "Minimal M0 model for ${config.nixfied.project.projectId}.";
    };
  };
}
