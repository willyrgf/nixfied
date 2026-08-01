# Batched Nix-layer negative cases for gate-nix.
#
# Each case must fail during Nix evaluation. Model cases force the compiled
# derivation; help cases force the complete rendered string. The result lists
# cases that unexpectedly evaluated.
{ checkout }:

let
  flake = builtins.getFlake (toString checkout);
  compileModel = (builtins.getAttr builtins.currentSystem flake.lib).compileModel;
  projectApps = (builtins.getAttr builtins.currentSystem flake.lib).projectApps;
  composite = checkout + /examples/composite/nixfied.nix;

  compiles = module: (builtins.tryEval ((compileModel module).drvPath)).success;
  projects = module: (builtins.tryEval ((projectApps module).help.program)).success;
  renderHelp = import (checkout + /nix/help-renderer.nix);
  renders = apps: (builtins.tryEval (builtins.stringLength (renderHelp apps))).success;

  reject = name: module: if compiles module then [ name ] else [ ];
  rejectProjectApps = name: module: if projects module then [ name ] else [ ];
  rejectHelp = name: apps: if renders apps then [ name ] else [ ];

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
    (rejectHelp "a help app whose program does not evaluate" {
      broken = {
        program = throw "program must be forced";
        meta.description = "Broken app";
      };
    })

    (rejectHelp "a help app with an empty program" {
      broken = {
        program = "";
        meta.description = "Broken app";
      };
    })

    (rejectHelp "a help app with a non-string program" {
      broken = {
        program = [ "/bin/false" ];
        meta.description = "Broken app";
      };
    })

    (rejectHelp "a help app without a description" {
      broken.program = "/bin/false";
    })

    (rejectHelp "a help app with an empty description" {
      broken = {
        program = "/bin/false";
        meta.description = "";
      };
    })

    (rejectHelp "a help app with a non-string description" {
      broken = {
        program = "/bin/false";
        meta.description = [ "invalid" ];
      };
    })

    (rejectProjectApps "projectApps called with a function module" (
      { ... }:
      {
        imports = [ composite ];
      }
    ))

    (rejectProjectApps "projectApps called with an attrset module" {
      imports = [ composite ];
    })

    (rejectProjectApps "projectApps called with a wrongly named root module" (checkout + /flake.nix))

    (rejectProjectApps "projectApps called with a nested module path" composite)

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

    (reject "the removed cacheEnv option" (
      { ... }:
      {
        imports = [ composite ];
        nixfied.tasks.smoke.invocation.cacheEnv.CARGO_TARGET_DIR = {
          family = "cargo-target";
          mode = "fast-dev";
          key.parts = [ "cache-v1" ];
        };
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

    (reject "a task whose endpoint demand exceeds its port window" (
      { lib, ... }:
      {
        imports = [ composite ];
        nixfied.placement.ports.windowSize = 1;
        nixfied.services.synthetic.endpoint = lib.mkForce null;
        nixfied.services.synthetic.endpoints = {
          api = { };
          admin = { };
        };
        nixfied.services.synthetic.primaryEndpoint = "api";
      }
    ))

    (reject "a dangling surface verb" (
      { ... }:
      {
        imports = [ composite ];
        nixfied.surface.verbs.ghost = "Run the missing task";
      }
    ))

    (reject "an empty surface verb description" (
      { ... }:
      {
        imports = [ composite ];
        nixfied.surface.verbs.smoke = "";
      }
    ))

    (reject "a non-string surface verb description" (
      { ... }:
      {
        imports = [ composite ];
        nixfied.surface.verbs.smoke = [ "invalid" ];
      }
    ))

    (reject "a surface verb description that is not forced" (
      { ... }:
      {
        imports = [ composite ];
        nixfied.surface.verbs.smoke = throw "surface description must be forced";
      }
    ))

    (reject "the removed list form of surface verbs" (
      { ... }:
      {
        imports = [ composite ];
        nixfied.surface.verbs = [ "smoke" ];
      }
    ))

    (reject "a surface verb colliding with the project-app namespace" (
      { lib, ... }:
      {
        imports = [ composite ];
        nixfied.closures.synthetic-helper.operationBindings = lib.mkForce null;
        nixfied.tasks.help = {
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
        nixfied.surface.verbs.help = "Run the reserved task";
      }
    ))

    (reject "immutable sourceIdentity outside the Nix store" (
      { ... }:
      {
        imports = [ composite ];
        nixfied.codebases.main.sourceMode = "snapshot";
        nixfied.codebases.main.sourceIdentity = "/tmp/nixfied-source";
        nixfied.codebases.main.dirtyPolicy = "reject";
      }
    ))

    (reject "live workspace dirtyPolicy reject" (
      { ... }:
      {
        imports = [ composite ];
        nixfied.codebases.main.dirtyPolicy = "reject";
      }
    ))

    (reject "an env-var secret resolver carrying a file path" (
      { ... }:
      {
        imports = [ composite ];
        nixfied.secrets.api-token.source = {
          kind = "env-var";
          envVar = "API_TOKEN";
          path = "api-token";
        };
      }
    ))

    (reject "a file secret resolver escaping the secrets dir" (
      { ... }:
      {
        imports = [ composite ];
        nixfied.secrets.api-token.source = {
          kind = "file";
          path = "../api-token";
        };
      }
    ))

    (reject "a secret placeholder outside env" (
      { lib, ... }:
      {
        imports = [ composite ];
        nixfied.secrets.api-token.source = {
          kind = "env-var";
          envVar = "API_TOKEN";
        };
        nixfied.tasks.smoke.invocation.run = lib.mkForce [
          "nixfied-synthetic-helper"
          "task"
          "--host"
          "127.0.0.1"
          "--port"
          "\${secret:api-token}"
        ];
      }
    ))

    (reject "an undeclared secret placeholder" (
      { ... }:
      {
        imports = [ composite ];
        nixfied.tasks.smoke.invocation.env.API_TOKEN = "\${secret:missing}";
      }
    ))

    (reject "a malformed secret placeholder" (
      { ... }:
      {
        imports = [ composite ];
        nixfied.secrets.api-token.source = {
          kind = "env-var";
          envVar = "API_TOKEN";
        };
        nixfied.tasks.smoke.invocation.env.API_TOKEN = "\${secret:api-token";
      }
    ))
  ]
