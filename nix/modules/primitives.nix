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

  operationId = mkOption {
    type = types.nullOr types.nonEmptyStr;
    default = null;
    description = "Globally unique lifecycle operation identifier; derived (`service.<name>.<op>`) unless overridden.";
  };
  # Terminal tokens default per lifecycle class (docs/DERIVATION_SPEC.md §5.2);
  # declare to override.
  terminalDefaults = (import ../lib/derive-facts.nix { inherit lib; }).terminalDefaults;
  mkTerminal = class: mkOption {
    type = terminalType;
    default = terminalDefaults.${class};
    description = "Typed terminal result tokens (defaulted per lifecycle class).";
  };
  # The one way anything in the model says "run this program" (INVOKE-1):
  # inline, anonymous, fully applied. `tools` is the tool set whose bin roots
  # form the child PATH: declared closure ids (strings) or plain packages the
  # compiler synthesizes tool closures from. `run` is the argv; `run[0]` is
  # resolved against the tool set at eval, so the runtime resolves nothing on
  # the host (SEAM-1).
  invocationOptions = {
    tools = mkOption {
      type = types.nonEmptyListOf (types.either types.nonEmptyStr types.package);
      description = "Tool set: declared closure ids or packages; their bin roots form the child PATH in order.";
    };
    run = mkOption {
      type = types.nonEmptyListOf types.str;
      description = "Argv. run[0] must be the executable basename of one tool.";
    };
    env = mkOption {
      type = types.attrsOf types.str;
      default = { };
      description = "Declared child environment (hermetic: nothing else is inherited; PATH is runtime-owned).";
    };
    codebaseId = mkOption {
      type = types.nonEmptyStr;
      default = "main";
      description = "Codebase the invocation observes.";
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
      description = "Invocation timeout.";
    };
  };
  invocationType = types.submodule { options = invocationOptions; };

  # Each lifecycle class binds exactly the primitive its mechanism needs: an
  # illegal binding (a stop invocation, a probe on start) is unrepresentable.
  # prepare binds a task reference with full task semantics: composites
  # allowed, cross-service `requires` allowed (the combined connectsTo +
  # prepare-requires graph must stay acyclic). Its evidence is the referenced
  # task's flattened nodes.
  prepareOpType = types.submodule {
    options = {
      task = mkOption {
        type = types.nullOr types.nonEmptyStr;
        default = null;
        description = "The declared task (leaf or composite) that prepares this service's state.";
      };
    };
  };
  startOpType = types.submodule {
    options = {
      inherit operationId;
      terminal = mkTerminal "start";
      invocation = mkOption {
        type = invocationType;
        description = "Invocation spawned and owned as the foreground service.";
      };
    };
  };
  probeSpecType = types.submodule {
    options = {
      kind = mkOption {
        type = types.enum [
          "tcp"
          "exec"
        ];
        default = "tcp";
        description = "Probe mechanism: tcp-connect the service endpoint, or run a bound short-lived invocation (exit 0 = success).";
      };
      invocation = mkOption {
        type = types.nullOr invocationType;
        default = null;
        description = "The invocation an `exec` probe runs (e.g. pg_isready); must be null for `tcp`.";
      };
      timeoutMs = mkOption {
        type = positiveInt;
        default = 1000;
        description = "Per-attempt probe timeout (the invocation's own timeoutMs does not apply to probe attempts).";
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
  probeOpType = class: types.submodule {
    options = {
      inherit operationId;
      terminal = mkTerminal class;
      probe = mkOption {
        type = probeSpecType;
        default = { };
        description = "How the op decides the service answers: a tcp-connect of the endpoint, or a bound exec probe.";
      };
    };
  };
  stopOpType = types.submodule {
    options = {
      inherit operationId;
      terminal = mkTerminal "stop";
      signal = mkOption {
        type = types.enum [
          "TERM"
          "INT"
          "QUIT"
          "HUP"
        ];
        default = "TERM";
        description = "Graceful stop signal.";
      };
      timeoutMs = mkOption {
        type = positiveInt;
        default = 5000;
        description = "Graceful stop timeout before SIGKILL escalation.";
      };
    };
  };
  cleanOpType = types.submodule {
    options = {
      inherit operationId;
      terminal = mkTerminal "clean";
    };
  };
  lifecycleType = types.submodule {
    options = {
      prepare = mkOption {
        type = prepareOpType;
        default = { };
        description = "Optional data-dir init.";
      };
      start = mkOption {
        type = startOpType;
        description = "Spawn-and-own the foreground service.";
      };
      ready = mkOption {
        type = probeOpType "ready";
        default = { };
        description = "Wait on the readiness probe.";
      };
      health = mkOption {
        type = probeOpType "health";
        default = { };
        description = "Wait on a health probe.";
      };
      stop = mkOption {
        type = stopOpType;
        default = { };
        description = "Signal-based graceful shutdown.";
      };
      clean = mkOption {
        type = cleanOpType;
        default = { };
        description = "Marker-gated runtime cleanup.";
      };
    };
  };

  endpointType = types.submodule {
    options = {
      endpointId = mkOption {
        type = types.nonEmptyStr;
        description = "Stable logical endpoint identifier.";
      };
      host = mkOption {
        type = types.nonEmptyStr;
        default = "127.0.0.1";
        description = "Endpoint loopback bind host.";
      };
    };
  };

  # A multi-endpoint service keys its endpoints by id (the attr name), so the
  # submodule carries only the bind host.
  namedEndpointType = types.submodule {
    options = {
      host = mkOption {
        type = types.nonEmptyStr;
        default = "127.0.0.1";
        description = "Endpoint loopback bind host.";
      };
    };
  };

  serviceType = types.submodule {
    options = {
      lifecycle = mkOption {
        type = lifecycleType;
        description = "Full generic lifecycle operation contract.";
      };
      endpoint = mkOption {
        type = types.nullOr endpointType;
        default = null;
        description = ''
          Single-endpoint sugar: the one tcp loopback endpoint the service binds.
          Mutually exclusive with `endpoints`/`primaryEndpoint`; at most one form
          may be set — a service with neither is endpoint-less (durable is not
          listening): owned, invocation-probed, contained, and cleaned, but not
          addressable.
        '';
      };
      endpoints = mkOption {
        type = types.attrsOf namedEndpointType;
        default = { };
        description = ''
          The tcp loopback endpoints the service binds, keyed by endpointId. The
          planner reserves a contiguous port block — one port per endpoint — so
          every listener is modelled and conflict-checked. Own endpoints are
          addressable in exec args/env via ''${port:<endpointId>} and
          ''${host:<endpointId>}. Set `primaryEndpoint` to name the primary.
        '';
      };
      primaryEndpoint = mkOption {
        type = types.nullOr types.nonEmptyStr;
        default = null;
        description = ''
          The endpoint bare ''${port}/''${host} resolve to, the tcp readiness/health
          probe target, and the endpoint a connectsTo dependent reaches by service
          id. Required with `endpoints`; must name one of its keys.
        '';
      };
      connectsTo = mkOption {
        type = types.listOf types.nonEmptyStr;
        default = [ ];
        description = ''
          Services this service connects to in the same slot. Declaring a
          dependency makes its endpoint addressable from this service's exec
          args and env via ''${port:<serviceId>} and ''${host:<serviceId>},
          and orders service startup so dependencies are ready first.
        '';
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
        type = types.nullOr (types.listOf types.str);
        default = null;
        description = ''
          Optional narrowing gate: the operation ids this closure may be
          dispatched against. The model carries the *derived* bindings (from
          the invocation graph); declaring a list additionally requires the
          derived set to be a subset of it.
        '';
      };
      effects = mkOption {
        type = types.listOf (
          types.enum [
            "process"
            "network-listener"
            "source-read"
            "file-write"
          ]
        );
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

  stepType = types.submodule {
    options = {
      task = mkOption {
        type = types.nonEmptyStr;
        description = "The task (leaf or composite) this step runs.";
      };
      dependsOn = mkOption {
        type = types.listOf types.str;
        default = [ ];
        description = "Sibling steps that must have completed successfully before this step starts.";
      };
    };
  };

  # A task is a leaf (one bounded invocation + orchestration) or a composite (a
  # static named-step DAG over task references). Kind/field coherence is
  # validated at eval; the runtime re-proves it at admission.
  taskType = types.submodule {
    options = {
      kind = mkOption {
        type = types.enum [
          "leaf"
          "composite"
        ];
        default = "leaf";
        description = "Task kind: a bounded leaf invocation, or a composite DAG of steps.";
      };
      operationId = mkOption {
        type = types.nullOr types.nonEmptyStr;
        default = null;
        description = "Globally unique task operation identifier (leaf only); derived (`task.<name>.run`) unless overridden.";
      };
      invocation = mkOption {
        type = types.nullOr invocationType;
        default = null;
        description = "The leaf invocation the task runs (leaf only).";
      };
      steps = mkOption {
        type = types.attrsOf stepType;
        default = { };
        description = "Composite body: named steps referencing declared tasks (composite only).";
      };
      requires = mkOption {
        type = types.listOf types.str;
        default = [ ];
        description = "Services that must be ready (alive, probed, addressable) while the leaf runs.";
      };
      exitPolicy = mkOption {
        type = exitPolicyType;
        default = { };
        description = "Task exit policy.";
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
      default = 23080;
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

}
