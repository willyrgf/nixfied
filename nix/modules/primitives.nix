{ lib, ... }:
let
  inherit (lib) mkOption types;
  positiveInt = types.addCheck types.int (value: value > 0);
  port = types.addCheck types.int (value: value >= 1 && value <= 65535);

  terminalType = types.submodule {
    options = {
      success = mkOption {
        type = types.nonEmptyStr;
        description = "Terminal result token recorded on operation success.";
      };
      failure = mkOption {
        type = types.nonEmptyStr;
        description = "Terminal result token recorded on operation failure.";
      };
    };
  };

  lifecycleOpType = types.submodule {
    options = {
      operationId = mkOption {
        type = types.nonEmptyStr;
        description = "Globally unique lifecycle operation identifier.";
      };
      class = mkOption {
        type = types.enum [
          "prepare"
          "start"
          "ready"
          "health"
          "stop"
          "clean"
        ];
        description = "Generic lifecycle operation class.";
      };
      execId = mkOption {
        type = types.nullOr types.str;
        default = null;
        description = "Exec bound to this operation, when it executes a command.";
      };
      execArgs = mkOption {
        type = types.listOf types.str;
        default = [ ];
        description = "Operation-specific args appended to the exec args.";
      };
      probeId = mkOption {
        type = types.nullOr types.str;
        default = null;
        description = "Probe bound to this operation, for readiness/health.";
      };
      terminal = mkOption {
        type = terminalType;
        description = "Typed terminal result tokens.";
      };
    };
  };

  endpointType = types.submodule {
    options = {
      endpointId = mkOption {
        type = types.nonEmptyStr;
        description = "Stable logical endpoint identifier.";
      };
      protocol = mkOption {
        type = types.enum [ "tcp" ];
        default = "tcp";
        description = "Endpoint protocol.";
      };
      host = mkOption {
        type = types.nonEmptyStr;
        default = "127.0.0.1";
        description = "Endpoint bind host.";
      };
      ownershipVerification = mkOption {
        type = types.enum [ "required" ];
        default = "required";
        description = "Endpoint ownership verification policy.";
      };
      socketActivation = mkOption {
        type = types.enum [ "disabled" ];
        default = "disabled";
        description = "Socket activation policy.";
      };
    };
  };

  probeTargetType = types.submodule {
    options = {
      kind = mkOption {
        type = types.enum [
          "tcp-connect"
          "http-get"
        ];
        description = "Probe target kind.";
      };
      endpointId = mkOption {
        type = types.nonEmptyStr;
        description = "Endpoint the probe observes.";
      };
      path = mkOption {
        type = types.nullOr types.str;
        default = null;
        description = "HTTP path for http-get probes.";
      };
    };
  };

  probeType = types.submodule {
    options = {
      probeId = mkOption {
        type = types.nonEmptyStr;
        description = "Stable probe identifier.";
      };
      target = mkOption {
        type = probeTargetType;
        description = "Probe target.";
      };
      timeoutMs = mkOption {
        type = positiveInt;
        default = 1000;
        description = "Per-attempt probe timeout.";
      };
      retryIntervalMs = mkOption {
        type = positiveInt;
        default = 100;
        description = "Probe retry interval.";
      };
      maxAttempts = mkOption {
        type = positiveInt;
        default = 20;
        description = "Maximum probe attempts.";
      };
    };
  };

  stopPolicyType = types.submodule {
    options = {
      signal = mkOption {
        type = types.nonEmptyStr;
        default = "TERM";
        description = "Stop signal.";
      };
      timeoutMs = mkOption {
        type = positiveInt;
        default = 5000;
        description = "Graceful stop timeout.";
      };
    };
  };

  serviceType = types.submodule {
    options = {
      foreground = mkOption {
        type = types.bool;
        default = true;
        description = "Whether the service runs in the runtime-owned foreground process group.";
      };
      lifecycle = mkOption {
        type = types.listOf lifecycleOpType;
        description = "Full generic lifecycle operation contract.";
      };
      endpoints = mkOption {
        type = types.listOf endpointType;
        description = "Service endpoints.";
      };
      probes = mkOption {
        type = types.listOf probeType;
        description = "Readiness/health probes.";
      };
      readinessProbe = mkOption {
        type = types.nonEmptyStr;
        description = "Probe id used to verify readiness.";
      };
      healthPolicy = mkOption {
        type = types.enum [
          "explicit"
          "unsupported"
        ];
        default = "explicit";
        description = "Health policy.";
      };
      stopPolicy = mkOption {
        type = stopPolicyType;
        default = { };
        description = "Stop policy.";
      };
      stateRefs = mkOption {
        type = types.listOf types.str;
        default = [ "slot" ];
        description = "State roots the service owns.";
      };
      logRefs = mkOption {
        type = types.listOf types.str;
        default = [ ];
        description = "Log roots the service writes.";
      };
      containment = mkOption {
        type = types.enum [
          "process-group"
          "process-tree"
        ];
        default = "process-group";
        description = "Containment requirement.";
      };
      lifetime = mkOption {
        type = types.enum [ "run-scoped" ];
        default = "run-scoped";
        description = "Service lifetime policy.";
      };
    };
  };

  execType = types.submodule {
    options = {
      closureId = mkOption {
        type = types.nonEmptyStr;
        description = "Closure providing the executable.";
      };
      executable = mkOption {
        type = types.nullOr types.str;
        default = null;
        description = "Absolute executable path; defaults to the closure executable.";
      };
      args = mkOption {
        type = types.listOf types.str;
        default = [ ];
        description = "Base exec args.";
      };
      env = mkOption {
        type = types.attrsOf types.str;
        default = { };
        description = "Exec environment variables.";
      };
      codebaseId = mkOption {
        type = types.nonEmptyStr;
        default = "main";
        description = "Codebase the exec observes.";
      };
      cwd = mkOption {
        type = types.nonEmptyStr;
        default = ".";
        description = "Confined relative working directory under the codebase.";
      };
      stdin = mkOption {
        type = types.enum [
          "null"
          "inherit"
        ];
        default = "null";
        description = "Stdin policy.";
      };
      timeoutMs = mkOption {
        type = positiveInt;
        default = 30000;
        description = "Exec timeout.";
      };
      outputCapture = mkOption {
        type = types.enum [
          "none"
          "stdout"
          "stderr"
          "stdout-stderr"
        ];
        default = "stdout-stderr";
        description = "Output capture policy.";
      };
      cancellationMode = mkOption {
        type = types.enum [
          "kill-process-group"
          "kill-process"
        ];
        default = "kill-process-group";
        description = "Cancellation mode.";
      };
    };
  };

  closureType = types.submodule {
    options = {
      package = mkOption {
        type = types.package;
        description = "Realised Nix package providing the closure.";
      };
      executable = mkOption {
        type = types.nonEmptyStr;
        description = "Executable path relative to the package store path.";
      };
      kind = mkOption {
        type = types.enum [
          "executable"
          "helper"
        ];
        default = "executable";
        description = "Closure kind.";
      };
      requiresExecutable = mkOption {
        type = types.bool;
        default = true;
        description = "Whether the runtime must verify the executable bit.";
      };
      operationBindings = mkOption {
        type = types.listOf types.str;
        default = [ ];
        description = "Lifecycle/task operation ids dispatched against this closure.";
      };
      effects = mkOption {
        type = types.listOf (types.enum [
          "process"
          "network-listener"
          "source-read"
          "file-write"
        ]);
        default = [ "process" ];
        description = "Declared closure effects.";
      };
    };
  };

  exitPolicyType = types.submodule {
    options = {
      successCodes = mkOption {
        type = types.listOf types.int;
        default = [ 0 ];
        description = "Exit codes treated as success.";
      };
    };
  };

  taskType = types.submodule {
    options = {
      operationId = mkOption {
        type = types.nonEmptyStr;
        description = "Globally unique task operation identifier.";
      };
      execId = mkOption {
        type = types.nonEmptyStr;
        description = "Exec the task runs.";
      };
      args = mkOption {
        type = types.listOf types.str;
        default = [ ];
        description = "Task-specific args appended to the exec args.";
      };
      dependsOnServicesReady = mkOption {
        type = types.listOf types.str;
        default = [ ];
        description = "Services that must be ready before the task runs.";
      };
      exitPolicy = mkOption {
        type = exitPolicyType;
        default = { };
        description = "Task exit policy.";
      };
      outputCapture = mkOption {
        type = types.enum [
          "none"
          "stdout"
          "stderr"
          "stdout-stderr"
        ];
        default = "stdout-stderr";
        description = "Output capture policy.";
      };
      artifactRefs = mkOption {
        type = types.listOf types.str;
        default = [ ];
        description = "Artifact roots the task writes.";
      };
      logRefs = mkOption {
        type = types.listOf types.str;
        default = [ ];
        description = "Log roots the task writes.";
      };
      summaryRefs = mkOption {
        type = types.listOf types.str;
        default = [ "summary" ];
        description = "Summary roots the task writes.";
      };
    };
  };
in
{
  options.nixfied.closures = mkOption {
    type = types.attrsOf closureType;
    default = { };
    description = "Realised runtime closures, keyed by closure id.";
  };

  options.nixfied.execs = mkOption {
    type = types.attrsOf execType;
    default = { };
    description = "Reusable exec specs, keyed by exec id.";
  };

  options.nixfied.services = mkOption {
    type = types.attrsOf serviceType;
    default = { };
    description = "Declared services, keyed by service id.";
  };

  options.nixfied.tasks = mkOption {
    type = types.attrsOf taskType;
    default = { };
    description = "Declared bounded tasks, keyed by task id.";
  };

  options.nixfied.placement.ports = {
    base = mkOption {
      type = port;
      default = 38080;
      description = "Base TCP port for the first slot's candidate window.";
    };

    windowSize = mkOption {
      type = positiveInt;
      default = 11;
      description = "Number of candidate ports assigned to each slot.";
    };

    slotStride = mkOption {
      type = positiveInt;
      default = 100;
      description = "Port offset between adjacent slot candidate windows.";
    };
  };

  options.nixfied.secrets = mkOption {
    type = types.listOf types.attrs;
    default = [ ];
    description = "Secret descriptors are still deferred; must remain empty.";
  };

  options.nixfied.workflows = mkOption {
    type = types.attrs;
    default = { };
    description = "Workflow declarations are deferred to M4; must remain empty.";
  };
}
