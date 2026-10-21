# Batched Nix-layer negative cases for gate-nix.
#
# Rejection cases must fail during Nix evaluation; supported module forms must
# construct help. Manifest cases force the compiled derivation; help cases force
# the complete rendered string. The result lists violated expectations.
{ checkout }:

let
  flake = builtins.getFlake (toString checkout);
  lib = flake.inputs.nixpkgs.lib;
  compileManifest = (builtins.getAttr builtins.currentSystem flake.lib).compileManifest;
  projectApps = (builtins.getAttr builtins.currentSystem flake.lib).projectApps;
  composite = checkout + /examples/composite/nixfied.nix;

  compiles = module: (builtins.tryEval ((compileManifest module).drvPath)).success;
  projects = module: (builtins.tryEval ((projectApps module).help.program)).success;
  renderHelp = import (checkout + /nix/help-renderer.nix);
  renders = apps: (builtins.tryEval (builtins.stringLength (renderHelp apps))).success;

  reject = name: module: if compiles ({ imports = [ composite ]; } // module) then [ name ] else [ ];
  acceptProjectApps = name: module: if projects module then [ ] else [ name ];
  rejectHelp = name: apps: if renders apps then [ name ] else [ ];
  invocation = mode: {
    tools = [ "synthetic-helper" ];
    run = [
      "nixfied-synthetic-helper"
      mode
      "--host"
      "127.0.0.1"
      "--port"
      "1"
    ];
  };
  taskInvocation = invocation "task";
  endpointless = {
    imports = [ composite ];
    nixfied.services.synthetic = {
      endpoint = lib.mkForce null;
      lifecycle.ready.probe = taskInvocation;
      lifecycle.health.probe = taskInvocation;
    };
  };

  validCompositeCompiles = compiles (
    { ... }:
    {
      imports = [ composite ];
    }
  );
in
if !validCompositeCompiles then
  throw "gate-nix negatives sanity check failed: valid composite manifest did not compile"
else
  builtins.concatLists [
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

    (acceptProjectApps "projectApps called with a function module" (
      { ... }:
      {
        imports = [ composite ];
      }
    ))

    (acceptProjectApps "projectApps called with an attrset module" {
      imports = [ composite ];
    })

    (acceptProjectApps "projectApps called with a nested module path" composite)

    (reject "an undeclared step task" {
      nixfied.tasks.pipeline.steps.bad.task = "missing-task";
    })

    (reject "dependsOn an unknown step" {
      nixfied.tasks.pipeline.steps.verify.dependsOn = lib.mkForce [ "ghost-step" ];
    })

    (reject "an empty composite" {
      nixfied.tasks.pipeline.steps = lib.mkForce { };
    })

    (reject "a cyclic step graph" {
      nixfied.tasks.pipeline.steps.probe.dependsOn = lib.mkForce [ "verify" ];
    })

    (reject "a self-cycle in a step graph" {
      nixfied.tasks.pipeline.steps.probe.dependsOn = lib.mkForce [ "probe" ];
    })

    (reject "a self-cycle in connectsTo" {
      nixfied.services.synthetic.connectsTo = [ "synthetic" ];
    })

    (reject "a runtime-owned PATH declared in env" {
      nixfied.tasks.smoke.invocation.env.PATH = "/usr/bin";
    })

    (reject "the removed cacheEnv option" {
      nixfied.tasks.smoke.invocation.cacheEnv.CARGO_TARGET_DIR = {
        family = "cargo-target";
        mode = "fast-dev";
        key.parts = [ "cache-v1" ];
      };
    })

    (reject "an unresolvable run[0]" {
      nixfied.tasks.smoke.invocation.run = lib.mkForce [ "ghost-program" ];
    })

    (reject "a dotted task id (step-path discipline)" {
      nixfied.tasks."has.dot" = {
        invocation = taskInvocation;
      };
    })

    (reject "a leaf carrying steps" {
      nixfied.tasks.smoke.steps.bad.task = "smoke";
    })

    (reject "a composite carrying an invocation" {
      nixfied.tasks.pipeline.invocation = {
        tools = [ "synthetic-helper" ];
        run = [ "nixfied-synthetic-helper" ];
      };
    })

    (reject "a composite carrying task-output as its default" {
      nixfied.tasks.pipeline.defaultOutput = "task-output";
    })

    (reject "a cyclic task reference graph" {
      nixfied.tasks.loop-a = {
        kind = "composite";
        steps.next.task = "loop-b";
      };
      nixfied.tasks.loop-b = {
        kind = "composite";
        steps.next.task = "loop-a";
      };
    })

    (reject "a self-cycle in task references" {
      nixfied.tasks.pipeline.steps.loop.task = "pipeline";
    })

    (reject "a duplicate effective operation id" {
      nixfied.tasks.smoke.operationId = "service.synthetic.start";
    })

    (reject "a task named endpoint ref outside requires" {
      nixfied.tasks.smoke.invocation.run = lib.mkForce [
        "nixfied-synthetic-helper"
        "task"
        "--host"
        "127.0.0.1"
        "--port"
        "\${port:ghost}"
      ];
    })

    (reject "a task using an endpoint id instead of a required service id" {
      nixfied.tasks.smoke.invocation.run = lib.mkForce [
        "nixfied-synthetic-helper"
        "task"
        "--port"
        "\${port:synthetic-tcp}"
      ];
    })

    (reject "a bare task placeholder with an endpoint-less first dependency" {
      imports = [ (checkout + /examples/toolchain/nixfied.nix) ];
      nixfied.tasks.db-test.requires = lib.mkForce [
        "worker"
        "postgres"
      ];
      nixfied.tasks.db-test.invocation.run = lib.mkForce [
        "psql"
        "-p"
        "\${port}"
      ];
    })

    (reject "a service named endpoint ref outside connectsTo" {
      nixfied.services.synthetic.lifecycle.start.invocation.run = lib.mkForce [
        "nixfied-synthetic-helper"
        "service"
        "--host"
        "127.0.0.1"
        "--port"
        "\${port:ghost}"
      ];
    })

    (reject "both endpoint forms set" {
      nixfied.services.synthetic.endpoints = {
        extra = { };
      };
      nixfied.services.synthetic.primaryEndpoint = "extra";
    })

    (reject "missing probes on an endpoint-less service" {
      nixfied.services.bare = {
        lifecycle.start.invocation = invocation "service";
      };
    })

    (reject "a listening service without the network-listener attestation" {
      nixfied.closures.synthetic-helper.effects = lib.mkForce [ "process" ];
    })

    (reject "an endpoint-less lifecycle using a bare endpoint placeholder" {
      imports = [ endpointless ];
      nixfied.closures.synthetic-helper.effects = lib.mkForce [ "process" ];
    })

    (reject "a task addressing an endpoint-less required service" {
      imports = [ endpointless ];
      nixfied.closures.synthetic-helper.effects = lib.mkForce [ "process" ];
      nixfied.services.synthetic.lifecycle.start.invocation.run = lib.mkForce taskInvocation.run;
      nixfied.tasks.smoke.invocation.run = lib.mkForce [
        "nixfied-synthetic-helper"
        "task"
        "--host"
        "127.0.0.1"
        "--port"
        "\${port:synthetic}"
      ];
    })

    (reject "an endpoint-less service start declaring network-listener" {
      imports = [ endpointless ];
      nixfied.services.synthetic.lifecycle.start.invocation.run = lib.mkForce taskInvocation.run;
    })

    (reject "a dangling prepare task" {
      nixfied.services.synthetic.lifecycle.prepare.task = "ghost";
    })

    (reject "a prepare requiring its own service (combined-graph cycle)" {
      nixfied.tasks.selfinit = {
        invocation = taskInvocation;
        requires = [ "synthetic" ];
      };
      nixfied.services.synthetic.lifecycle.prepare.task = "selfinit";
    })

    (reject "a task whose endpoint demand exceeds its port window" {
      nixfied.placement.ports.windowSize = 1;
      nixfied.services.synthetic.endpoint = lib.mkForce null;
      nixfied.services.synthetic.endpoints = {
        api = { };
        admin = { };
      };
      nixfied.services.synthetic.primaryEndpoint = "api";
    })

    (reject "a dangling surface verb" {
      nixfied.surface.verbs.ghost = "Run the missing task";
    })

    (reject "an empty surface verb description" {
      nixfied.surface.verbs.smoke = "";
    })

    (reject "a non-string surface verb description" {
      nixfied.surface.verbs.smoke = [ "invalid" ];
    })

    (reject "a surface verb description that is not forced" {
      nixfied.surface.verbs.smoke = throw "surface description must be forced";
    })

    (reject "the removed list form of surface verbs" {
      nixfied.surface.verbs = [ "smoke" ];
    })

    (reject "a surface verb colliding with the project-app namespace" {
      nixfied.tasks.help = {
        invocation = taskInvocation;
      };
      nixfied.surface.verbs.help = "Run the reserved task";
    })

    (reject "a task verb shadowing the docs app" {
      nixfied.tasks.docs.invocation = taskInvocation;
      nixfied.surface.verbs.docs = "Shadow framework reference";
    })

    (reject "immutable sourceIdentity outside the Nix store" {
      nixfied.codebases.main.sourceMode = "snapshot";
      nixfied.codebases.main.sourceIdentity = "/tmp/nixfied-source";
      nixfied.codebases.main.dirtyPolicy = "reject";
    })

    (reject "live workspace dirtyPolicy reject" {
      nixfied.codebases.main.dirtyPolicy = "reject";
    })

    (reject "an env-var secret resolver carrying a file path" {
      nixfied.secrets.api-token.source = {
        kind = "env-var";
        envVar = "API_TOKEN";
        path = "api-token";
      };
    })

    (reject "a file secret resolver escaping the secrets dir" {
      nixfied.secrets.api-token.source = {
        kind = "file";
        path = "../api-token";
      };
    })

    (reject "a secret placeholder outside env" {
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
    })

    (reject "an undeclared secret placeholder" {
      nixfied.tasks.smoke.invocation.env.API_TOKEN = "\${secret:missing}";
    })

    (reject "a malformed secret placeholder" {
      nixfied.secrets.api-token.source = {
        kind = "env-var";
        envVar = "API_TOKEN";
      };
      nixfied.tasks.smoke.invocation.env.API_TOKEN = "\${secret:api-token";
    })
  ]
