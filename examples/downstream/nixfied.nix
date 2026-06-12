# Downstream worked example: a realistic small system.
#
# This is the copy-paste adoption guide other teams start from. It composes a
# Postgres database (via the reusable `adapters.postgres` Nix-side adapter) with
# two first-party services written as plain Nix-built executables - an `api` and
# a `worker` - and orchestrates them with a `release` composite task. Everything below
# compiles into the generic model primitives; the runtime gains no knowledge of
# Postgres, the api, or the worker.
#
# Build it:    nix build ./examples/downstream#model
# Run it:      nixfied-runtime run --model <store>/model.json
{ pkgs, adapters, ... }:
let
  # A tiny TCP app used for both the api and the worker. In `service` mode it
  # listens and answers; in `task` mode it connects and prints the reply. Real
  # projects point closures at their own packaged binaries instead.
  appHelper = pkgs.writeTextFile {
    name = "downstream-app";
    destination = "/bin/downstream-app";
    executable = true;
    text = ''
      #!${pkgs.python3}/bin/python3
      import argparse, os, socket, sys

      def check_env(name):
          # A named endpoint placeholder that survives substitution would reach
          # us literally; treat that as a hard failure so the gate catches it.
          value = os.environ.get(name)
          if value is not None and "''${" in value:
              print(f"unsubstituted placeholder in {name}: {value}", file=sys.stderr)
              sys.exit(3)

      def touch(target):
          host, _, port = target.rpartition(":")
          with socket.create_connection((host, int(port)), timeout=5) as c:
              c.sendall(b"ping\n"); sys.stdout.write(c.recv(4096).decode())

      def serve(a):
          check_env("NIXFIED_DEMO_DSN")
          # The runtime starts connectsTo dependencies first, so an upstream
          # named by ''${host:..}:''${port:..} is already ready here.
          if a.upstream:
              touch(a.upstream)
          with socket.socket(socket.AF_INET, socket.SOCK_STREAM) as s:
              s.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1)
              s.bind((a.host, a.port)); s.listen()
              print(f"{a.label} listening on {a.host}:{a.port}", flush=True)
              while True:
                  c, _ = s.accept()
                  with c:
                      c.recv(4096); c.sendall(f"{a.label}-ok\n".encode())

      def ping(a):
          check_env("NIXFIED_DEMO_DSN")
          with socket.create_connection((a.host, a.port), timeout=5) as c:
              c.sendall(b"ping\n"); sys.stdout.write(c.recv(4096).decode())
          for target in a.also:
              touch(target)

      p = argparse.ArgumentParser()
      sub = p.add_subparsers(dest="cmd", required=True)
      for name in ("service", "task"):
          q = sub.add_parser(name)
          q.add_argument("--label", required=True)
          q.add_argument("--host", required=True)
          q.add_argument("--port", required=True, type=int)
          q.add_argument("--upstream", default=None)
          q.add_argument("--also", action="append", default=[])
      sub.add_parser("stop")
      a = p.parse_args()
      if a.cmd == "service": serve(a)
      elif a.cmd == "task": ping(a)
      else: sys.exit(0)
    '';
  };

  serviceRun = label: [
    "downstream-app"
    "service"
    "--label"
    label
    "--host"
    "127.0.0.1"
    "--port"
    "\${port}"
  ];
  taskRun = label: [
    "downstream-app"
    "task"
    "--label"
    label
    "--host"
    "127.0.0.1"
    "--port"
    "\${port}"
  ];

  # One service definition reused for the api and the worker. `connectsTo`
  # makes the named endpoints (`''${host:<serviceId>}`/`''${port:<serviceId>}`)
  # of same-slot dependencies addressable from the start args and orders
  # startup so they are ready first; `extraStartArgs` carries that wiring.
  mkAppService = name: connectsTo: extraStartArgs: {
    lifecycle = {
      start = {
        operationId = "service.${name}.start";
        invocation = {
          tools = [ "app" ];
          run = serviceRun name ++ extraStartArgs;
        };
        terminal = {
          success = "spawned";
          failure = "failed";
        };
      };
      ready = {
        operationId = "service.${name}.ready";
        terminal = {
          success = "ready";
          failure = "not-ready";
        };
      };
      health = {
        operationId = "service.${name}.health";
        terminal = {
          success = "healthy";
          failure = "unhealthy";
        };
      };
      stop = {
        operationId = "service.${name}.stop";
        terminal = {
          success = "stopped";
          failure = "failed";
        };
      };
      clean = {
        operationId = "service.${name}.clean";
        terminal = {
          success = "cleaned";
          failure = "failed";
        };
      };
    };
    endpoint = {
      endpointId = "${name}-tcp";
    };
    inherit connectsTo;
    logRefs = [ "service.${name}" ];
  };
in
{
  # Reuse the Postgres adapter for the database tier. It contributes the
  # `postgres` service and the `smoke-query` task (a `SELECT 1`), and adds them
  # to the `dev` environment; the declarations below merge with those.
  imports = [ adapters.postgres ];

  nixfied.project.projectId = "downstream";
  nixfied.project.name = "Downstream Worked Example";
  nixfied.codebases.main.logicalRoot = ".";

  # Two parallel slots of the same environment can run side by side; each gets a
  # disjoint port window, state root, and registry.
  nixfied.slotPolicy = {
    min = 0;
    default = 0;
    max = 1;
  };

  # A dedicated candidate window keeps this example off the default range.
  nixfied.placement.ports.base = 24880;

  nixfied.closures.app = {
    package = appHelper;
    executable = "bin/downstream-app";
    operationBindings = [
      "service.api.start"
      "service.worker.start"
      "task.ping-api.run"
      "task.ping-worker.run"
      "task.release-gate.run"
    ];
    effects = [
      "process"
      "network-listener"
    ];
  };

  nixfied.services.api = mkAppService "api" [ ] [ ];
  # The worker connects to the api in its own slot: the runtime starts the api
  # first and resolves the named endpoint from the slot plan.
  nixfied.services.worker =
    mkAppService "worker"
      [ "api" ]
      [
        "--upstream"
        "\${host:api}:\${port:api}"
      ];

  nixfied.tasks.ping-api = {
    operationId = "task.ping-api.run";
    invocation = {
      tools = [ "app" ];
      run = taskRun "api";
    };
    requires = [ "api" ];
    logRefs = [ "task.ping-api" ];
  };
  nixfied.tasks.ping-worker = {
    operationId = "task.ping-worker.run";
    invocation = {
      tools = [ "app" ];
      run = taskRun "worker";
    };
    requires = [ "worker" ];
    logRefs = [ "task.ping-worker" ];
  };
  # A release gate that requires the whole stack ready before it runs: it
  # depends on all three services (a task may depend on more than one). The
  # first dependency (api) is the primary providing bare ${port}/${host}; the
  # others are addressed by name via ${port:<serviceId>}/${host:<serviceId>},
  # and the env DSN proves env values are substituted too (the app fails on a
  # literal placeholder).
  nixfied.tasks.release-gate = {
    operationId = "task.release-gate.run";
    invocation = {
      tools = [ "app" ];
      run = taskRun "api" ++ [
        "--also"
        "\${host:worker}:\${port:worker}"
        "--also"
        "\${host:postgres}:\${port:postgres}"
      ];
      env = {
        NIXFIED_DEMO_DSN = "tcp://\${host:postgres}:\${port:postgres}";
      };
    };
    requires = [
      "api"
      "worker"
      "postgres"
    ];
    logRefs = [ "task.release-gate" ];
  };

  # A release flow: prove the database answers, then exercise both services,
  # then run the gate over the whole stack. A composite task; the services it
  # needs come from the environment until phase 3 derives them from the leaves.
  nixfied.tasks.release = {
    kind = "composite";
    steps = {
      db-check = {
        task = "smoke-query";
      };
      api-check = {
        task = "ping-api";
        dependsOn = [ "db-check" ];
      };
      worker-check = {
        task = "ping-worker";
        dependsOn = [ "db-check" ];
      };
      gate = {
        task = "release-gate";
        dependsOn = [
          "api-check"
          "worker-check"
        ];
      };
    };
  };
}
