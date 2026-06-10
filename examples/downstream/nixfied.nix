# Downstream worked example: a realistic small system.
#
# This is the copy-paste adoption guide other teams start from. It composes a
# Postgres database (via the reusable `adapters.postgres` Nix-side adapter) with
# two first-party services written as plain Nix-built executables - an `api` and
# a `worker` - and orchestrates them with a `release` workflow. Everything below
# compiles into the generic model primitives; the runtime gains no knowledge of
# Postgres, the api, or the worker.
#
# Build it:    nix build ./examples/downstream#model
# Run it:      nixfied-runtime run --model <store>/model.json
# Run a flow:  nixfied-runtime run --model <store>/model.json --workflow release
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
      import argparse, socket, sys

      def serve(a):
          with socket.socket(socket.AF_INET, socket.SOCK_STREAM) as s:
              s.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1)
              s.bind((a.host, a.port)); s.listen()
              print(f"{a.label} listening on {a.host}:{a.port}", flush=True)
              while True:
                  c, _ = s.accept()
                  with c:
                      c.recv(4096); c.sendall(f"{a.label}-ok\n".encode())

      def ping(a):
          with socket.create_connection((a.host, a.port), timeout=5) as c:
              c.sendall(b"ping\n"); sys.stdout.write(c.recv(4096).decode())

      p = argparse.ArgumentParser()
      sub = p.add_subparsers(dest="cmd", required=True)
      for name in ("service", "task"):
          q = sub.add_parser(name)
          q.add_argument("--label", required=True)
          q.add_argument("--host", required=True)
          q.add_argument("--port", required=True, type=int)
      sub.add_parser("stop")
      a = p.parse_args()
      if a.cmd == "service": serve(a)
      elif a.cmd == "task": ping(a)
      else: sys.exit(0)
    '';
  };

  serviceArgs = label: [
    "service"
    "--label"
    label
    "--host"
    "127.0.0.1"
    "--port"
    "\${port}"
  ];
  taskArgs = label: [
    "task"
    "--label"
    label
    "--host"
    "127.0.0.1"
    "--port"
    "\${port}"
  ];

  # One service definition reused for the api and the worker.
  mkAppService = name: {
    lifecycle = {
      prepare = {
        operationId = "service.${name}.prepare";
        terminal = {
          success = "prepared";
          failure = "failed";
        };
      };
      start = {
        operationId = "service.${name}.start";
        execId = name;
        execArgs = serviceArgs name;
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
  nixfied.placement.ports.base = 39880;

  nixfied.closures.app = {
    package = appHelper;
    executable = "bin/downstream-app";
    operationBindings = [
      "service.api.start"
      "service.worker.start"
      "task.ping-api.run"
      "task.ping-worker.run"
    ];
    effects = [
      "process"
      "network-listener"
    ];
  };

  nixfied.execs.api.closureId = "app";
  nixfied.execs.worker.closureId = "app";

  nixfied.services.api = mkAppService "api";
  nixfied.services.worker = mkAppService "worker";

  nixfied.tasks.ping-api = {
    operationId = "task.ping-api.run";
    execId = "api";
    args = taskArgs "api";
    dependsOnServicesReady = [ "api" ];
    logRefs = [ "task.ping-api" ];
  };
  nixfied.tasks.ping-worker = {
    operationId = "task.ping-worker.run";
    execId = "worker";
    args = taskArgs "worker";
    dependsOnServicesReady = [ "worker" ];
    logRefs = [ "task.ping-worker" ];
  };
  # A release gate that requires the whole stack ready before it runs: it depends
  # on all three services (a task may depend on more than one). The first
  # dependency (api) is the primary, providing ${port}; it pings the api once the
  # database and worker are also up.
  nixfied.tasks.release-gate = {
    operationId = "task.release-gate.run";
    execId = "api";
    args = taskArgs "api";
    dependsOnServicesReady = [
      "api"
      "worker"
      "postgres"
    ];
    logRefs = [ "task.release-gate" ];
  };

  # Add the api/worker to the `dev` environment (merges with the adapter's
  # `postgres` service and `smoke-query` task).
  nixfied.environments.dev = {
    services = [
      "api"
      "worker"
    ];
    tasks = [
      "ping-api"
      "ping-worker"
    ];
  };

  # A release flow: prove the database answers, then exercise both services.
  nixfied.workflows.release = {
    servicesRequired = [
      "postgres"
      "api"
      "worker"
    ];
    nodes = [
      {
        nodeId = "db-check";
        taskId = "smoke-query";
        dependsOn = [ ];
      }
      {
        nodeId = "api-check";
        taskId = "ping-api";
        dependsOn = [ "db-check" ];
      }
      {
        nodeId = "worker-check";
        taskId = "ping-worker";
        dependsOn = [ "db-check" ];
      }
      {
        nodeId = "gate";
        taskId = "release-gate";
        dependsOn = [
          "api-check"
          "worker-check"
        ];
      }
    ];
  };
}
