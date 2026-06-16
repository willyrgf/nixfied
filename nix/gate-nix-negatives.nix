# Batched Nix-layer negative cases for gate-nix.
#
# Each case must fail during evaluation of the compiled model derivation. The
# result is the list of case names that unexpectedly compiled.
{ checkout }:

let
  flake = builtins.getFlake (toString checkout);
  compileModel = (builtins.getAttr builtins.currentSystem flake.lib).compileModel;
  composite = checkout + /examples/composite/nixfied.nix;

  compiles = module: (builtins.tryEval ((compileModel module).drvPath)).success;

  reject = name: module: if compiles module then [ name ] else [ ];

  validCompositeCompiles = compiles (
    { ... }:
    {
      imports = [ composite ];
    }
  );
in
if !validCompositeCompiles then
  throw "gate-nix negatives sanity check failed: valid composite model did not compile"
else
  builtins.concatLists [
    (reject "an undeclared step task" (
      { ... }:
      {
        imports = [ composite ];
        nixfied.tasks.pipeline.steps.bad.task = "missing-task";
      }
    ))

    (reject "dependsOn an unknown step" (
      { lib, ... }:
      {
        imports = [ composite ];
        nixfied.tasks.pipeline.steps.verify.dependsOn = lib.mkForce [ "ghost-step" ];
      }
    ))

    (reject "an empty composite" (
      { lib, ... }:
      {
        imports = [ composite ];
        nixfied.tasks.pipeline.steps = lib.mkForce { };
      }
    ))

    (reject "a cyclic step graph" (
      { lib, ... }:
      {
        imports = [ composite ];
        nixfied.tasks.pipeline.steps.probe.dependsOn = lib.mkForce [ "verify" ];
      }
    ))

    (reject "a runtime-owned PATH declared in env" (
      { ... }:
      {
        imports = [ composite ];
        nixfied.tasks.smoke.invocation.env.PATH = "/usr/bin";
      }
    ))

    (reject "an unresolvable run[0]" (
      { lib, ... }:
      {
        imports = [ composite ];
        nixfied.tasks.smoke.invocation.run = lib.mkForce [ "ghost-program" ];
      }
    ))

    (reject "a dotted task id (step-path discipline)" (
      { lib, ... }:
      {
        imports = [ composite ];
        nixfied.closures.synthetic-helper.operationBindings = lib.mkForce null;
        nixfied.tasks."has.dot" = {
          invocation = {
            tools = [ "synthetic-helper" ];
            run = [
              "nixfied-synthetic-helper"
              "task"
              "--host"
              "127.0.0.1"
              "--port"
              "1"
            ];
          };
        };
      }
    ))

    (reject "a leaf carrying steps" (
      { ... }:
      {
        imports = [ composite ];
        nixfied.tasks.smoke.steps.bad.task = "smoke";
      }
    ))

    (reject "a composite carrying an invocation" (
      { ... }:
      {
        imports = [ composite ];
        nixfied.tasks.pipeline.invocation = {
          tools = [ "synthetic-helper" ];
          run = [ "nixfied-synthetic-helper" ];
        };
      }
    ))

    (reject "a cyclic task reference graph" (
      { ... }:
      {
        imports = [ composite ];
        nixfied.tasks.loop-a = {
          kind = "composite";
          steps.next.task = "loop-b";
        };
        nixfied.tasks.loop-b = {
          kind = "composite";
          steps.next.task = "loop-a";
        };
      }
    ))

    (reject "an operation binding gate narrower than the derivation" (
      { lib, ... }:
      {
        imports = [ composite ];
        nixfied.closures.synthetic-helper.operationBindings = lib.mkForce [ "task.smoke.run" ];
      }
    ))

    (reject "a duplicate effective operation id" (
      { ... }:
      {
        imports = [ composite ];
        nixfied.tasks.smoke.operationId = "service.synthetic.start";
      }
    ))

    (reject "a task named endpoint ref outside requires" (
      { lib, ... }:
      {
        imports = [ composite ];
        nixfied.tasks.smoke.invocation.run = lib.mkForce [
          "nixfied-synthetic-helper"
          "task"
          "--host"
          "127.0.0.1"
          "--port"
          "\${port:ghost}"
        ];
      }
    ))

    (reject "a service named endpoint ref outside connectsTo" (
      { lib, ... }:
      {
        imports = [ composite ];
        nixfied.services.synthetic.lifecycle.start.invocation.run = lib.mkForce [
          "nixfied-synthetic-helper"
          "service"
          "--host"
          "127.0.0.1"
          "--port"
          "\${port:ghost}"
        ];
      }
    ))

    (reject "both endpoint forms set" (
      { ... }:
      {
        imports = [ composite ];
        nixfied.services.synthetic.endpoints = {
          extra = { };
        };
        nixfied.services.synthetic.primaryEndpoint = "extra";
      }
    ))

    (reject "a tcp probe on an endpoint-less service" (
      { ... }:
      {
        imports = [ composite ];
        nixfied.services.bare = {
          lifecycle.start.invocation = {
            tools = [ "synthetic-helper" ];
            run = [
              "nixfied-synthetic-helper"
              "service"
              "--host"
              "127.0.0.1"
              "--port"
              "1"
            ];
          };
        };
      }
    ))

    (reject "a listening service without the network-listener attestation" (
      { lib, ... }:
      {
        imports = [ composite ];
        nixfied.closures.synthetic-helper.effects = lib.mkForce [ "process" ];
      }
    ))

    (reject "an endpoint-less lifecycle using a bare endpoint placeholder" (
      { lib, ... }:
      {
        imports = [ composite ];
        nixfied.closures.synthetic-helper.effects = lib.mkForce [ "process" ];
        nixfied.services.synthetic.endpoint = lib.mkForce null;
        nixfied.services.synthetic.lifecycle.ready.probe = {
          kind = "exec";
          invocation = {
            tools = [ "synthetic-helper" ];
            run = [
              "nixfied-synthetic-helper"
              "task"
              "--host"
              "127.0.0.1"
              "--port"
              "1"
            ];
          };
        };
        nixfied.services.synthetic.lifecycle.health.probe = {
          kind = "exec";
          invocation = {
            tools = [ "synthetic-helper" ];
            run = [
              "nixfied-synthetic-helper"
              "task"
              "--host"
              "127.0.0.1"
              "--port"
              "1"
            ];
          };
        };
      }
    ))

    (reject "a task addressing an endpoint-less required service" (
      { lib, ... }:
      {
        imports = [ composite ];
        nixfied.closures.synthetic-helper.effects = lib.mkForce [ "process" ];
        nixfied.services.synthetic.endpoint = lib.mkForce null;
        nixfied.services.synthetic.lifecycle.start.invocation.run = lib.mkForce [
          "nixfied-synthetic-helper"
          "task"
          "--host"
          "127.0.0.1"
          "--port"
          "1"
        ];
        nixfied.services.synthetic.lifecycle.ready.probe = {
          kind = "exec";
          invocation = {
            tools = [ "synthetic-helper" ];
            run = [
              "nixfied-synthetic-helper"
              "task"
              "--host"
              "127.0.0.1"
              "--port"
              "1"
            ];
          };
        };
        nixfied.services.synthetic.lifecycle.health.probe = {
          kind = "exec";
          invocation = {
            tools = [ "synthetic-helper" ];
            run = [
              "nixfied-synthetic-helper"
              "task"
              "--host"
              "127.0.0.1"
              "--port"
              "1"
            ];
          };
        };
        nixfied.tasks.smoke.invocation.run = lib.mkForce [
          "nixfied-synthetic-helper"
          "task"
          "--host"
          "127.0.0.1"
          "--port"
          "\${port:synthetic}"
        ];
      }
    ))

    (reject "an endpoint-less service start declaring network-listener" (
      { lib, ... }:
      {
        imports = [ composite ];
        nixfied.services.synthetic.endpoint = lib.mkForce null;
        nixfied.services.synthetic.lifecycle.start.invocation.run = lib.mkForce [
          "nixfied-synthetic-helper"
          "task"
          "--host"
          "127.0.0.1"
          "--port"
          "1"
        ];
        nixfied.services.synthetic.lifecycle.ready.probe = {
          kind = "exec";
          invocation = {
            tools = [ "synthetic-helper" ];
            run = [
              "nixfied-synthetic-helper"
              "task"
              "--host"
              "127.0.0.1"
              "--port"
              "1"
            ];
          };
        };
        nixfied.services.synthetic.lifecycle.health.probe = {
          kind = "exec";
          invocation = {
            tools = [ "synthetic-helper" ];
            run = [
              "nixfied-synthetic-helper"
              "task"
              "--host"
              "127.0.0.1"
              "--port"
              "1"
            ];
          };
        };
      }
    ))

    (reject "a dangling prepare task" (
      { ... }:
      {
        imports = [ composite ];
        nixfied.services.synthetic.lifecycle.prepare.task = "ghost";
      }
    ))

    (reject "a prepare requiring its own service (combined-graph cycle)" (
      { lib, ... }:
      {
        imports = [ composite ];
        nixfied.closures.synthetic-helper.operationBindings = lib.mkForce null;
        nixfied.tasks.selfinit = {
          invocation = {
            tools = [ "synthetic-helper" ];
            run = [
              "nixfied-synthetic-helper"
              "task"
              "--host"
              "127.0.0.1"
              "--port"
              "1"
            ];
          };
          requires = [ "synthetic" ];
        };
        nixfied.services.synthetic.lifecycle.prepare.task = "selfinit";
      }
    ))

    (reject "a dangling surface verb" (
      { ... }:
      {
        imports = [ composite ];
        nixfied.surface.verbs = [ "ghost" ];
      }
    ))

    (reject "a surface verb colliding with the control namespace" (
      { lib, ... }:
      {
        imports = [ composite ];
        nixfied.closures.synthetic-helper.operationBindings = lib.mkForce null;
        nixfied.tasks.clean = {
          invocation = {
            tools = [ "synthetic-helper" ];
            run = [
              "nixfied-synthetic-helper"
              "task"
              "--host"
              "127.0.0.1"
              "--port"
              "1"
            ];
          };
        };
        nixfied.surface.verbs = [ "clean" ];
      }
    ))

    (reject "live workspace dirtyPolicy reject" (
      { ... }:
      {
        imports = [ composite ];
        nixfied.codebases.main.dirtyPolicy = "reject";
      }
    ))
  ]
