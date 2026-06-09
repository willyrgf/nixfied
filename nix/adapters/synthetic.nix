# Synthetic service adapter.
#
# A Nix-side adapter is a library that compiles a concrete service into the
# generic model primitives. This one provides a self-contained foreground TCP
# service plus a dependent smoke task, used by the minimal example and proofs.
# The runtime gains no knowledge of it; everything here is generic data.
{ pkgs, ... }:
let
  helper = pkgs.writeTextFile {
    name = "nixfied-synthetic-helper";
    destination = "/bin/nixfied-synthetic-helper";
    executable = true;
    text = ''
      #!${pkgs.python3}/bin/python3

      import argparse
      import socket
      import sys


      def run_service(args):
          with socket.socket(socket.AF_INET, socket.SOCK_STREAM) as listener:
              listener.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1)
              listener.bind((args.host, args.port))
              listener.listen()
              print(
                  f"nixfied-synthetic-helper listening on {args.host}:{args.port}",
                  flush=True,
              )
              while True:
                  conn, _addr = listener.accept()
                  with conn:
                      data = conn.recv(4096)
                      if data.startswith(b"GET "):
                          conn.sendall(
                              b"HTTP/1.1 200 OK\r\ncontent-length: 2\r\n\r\nok"
                          )
                      else:
                          conn.sendall(b"ok\n")


      def run_task(args):
          with socket.create_connection((args.host, args.port), timeout=5) as conn:
              conn.sendall(b"smoke\n")
              sys.stdout.write(conn.recv(4096).decode("utf-8", "replace"))


      parser = argparse.ArgumentParser(prog="nixfied-synthetic-helper")
      sub = parser.add_subparsers(dest="command", required=True)
      service = sub.add_parser("service")
      service.add_argument("--host", required=True)
      service.add_argument("--port", required=True, type=int)
      task = sub.add_parser("task")
      task.add_argument("--host", required=True)
      task.add_argument("--port", required=True, type=int)
      stop = sub.add_parser("stop")
      args = parser.parse_args()

      if args.command == "service":
          run_service(args)
      elif args.command == "task":
          run_task(args)
      elif args.command == "stop":
          sys.exit(0)
    '';
  };
in
{
  nixfied.closures.synthetic-helper = {
    package = helper;
    executable = "bin/nixfied-synthetic-helper";
    kind = "executable";
    requiresExecutable = true;
    operationBindings = [
      "service.synthetic.start"
      "service.synthetic.stop"
      "task.smoke.run"
    ];
    effects = [
      "process"
      "network-listener"
    ];
  };

  nixfied.execs.synthetic-helper = {
    closureId = "synthetic-helper";
    codebaseId = "main";
    cwd = ".";
  };

  nixfied.services.synthetic = {
    foreground = true;
    readinessProbe = "synthetic-tcp";
    healthPolicy = "explicit";
    lifecycle = [
      {
        operationId = "service.synthetic.prepare";
        class = "prepare";
        terminal = {
          success = "prepared";
          failure = "failed";
        };
      }
      {
        operationId = "service.synthetic.start";
        class = "start";
        execId = "synthetic-helper";
        execArgs = [
          "service"
          "--host"
          "127.0.0.1"
          "--port"
          "\${port}"
        ];
        terminal = {
          success = "spawned";
          failure = "failed";
        };
      }
      {
        operationId = "service.synthetic.ready";
        class = "ready";
        probeId = "synthetic-tcp";
        terminal = {
          success = "ready";
          failure = "not-ready";
        };
      }
      {
        operationId = "service.synthetic.health";
        class = "health";
        probeId = "synthetic-tcp";
        terminal = {
          success = "healthy";
          failure = "unhealthy";
        };
      }
      {
        operationId = "service.synthetic.stop";
        class = "stop";
        execId = "synthetic-helper";
        execArgs = [ "stop" ];
        terminal = {
          success = "stopped";
          failure = "failed";
        };
      }
      {
        operationId = "service.synthetic.clean";
        class = "clean";
        terminal = {
          success = "cleaned";
          failure = "failed";
        };
      }
    ];
    endpoints = [
      {
        endpointId = "synthetic-tcp";
        protocol = "tcp";
        host = "127.0.0.1";
      }
    ];
    probes = [
      {
        probeId = "synthetic-tcp";
        target = {
          kind = "tcp-connect";
          endpointId = "synthetic-tcp";
        };
      }
    ];
    stateRefs = [ "slot" ];
    logRefs = [ "service.synthetic" ];
  };

  nixfied.tasks.smoke = {
    operationId = "task.smoke.run";
    execId = "synthetic-helper";
    args = [
      "task"
      "--host"
      "127.0.0.1"
      "--port"
      "\${port}"
    ];
    dependsOnServicesReady = [ "synthetic" ];
    logRefs = [ "task.smoke" ];
    summaryRefs = [ "summary" ];
  };

  nixfied.environments.dev = {
    services = [ "synthetic" ];
    tasks = [ "smoke" ];
  };
}
