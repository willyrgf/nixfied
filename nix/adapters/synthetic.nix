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
      args = parser.parse_args()

      if args.command == "service":
          run_service(args)
      elif args.command == "task":
          run_task(args)
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
      "task.smoke.run"
    ];
    effects = [
      "process"
      "network-listener"
    ];
  };

  nixfied.services.synthetic = {
    lifecycle = {
      prepare = {
        operationId = "service.synthetic.prepare";
        terminal = {
          success = "prepared";
          failure = "failed";
        };
      };
      start = {
        operationId = "service.synthetic.start";
        invocation = {
          tools = [ "synthetic-helper" ];
          run = [
            "nixfied-synthetic-helper"
            "service"
            "--host"
            "127.0.0.1"
            "--port"
            "\${port}"
          ];
        };
        terminal = {
          success = "spawned";
          failure = "failed";
        };
      };
      ready = {
        operationId = "service.synthetic.ready";
        terminal = {
          success = "ready";
          failure = "not-ready";
        };
      };
      health = {
        operationId = "service.synthetic.health";
        terminal = {
          success = "healthy";
          failure = "unhealthy";
        };
      };
      stop = {
        operationId = "service.synthetic.stop";
        terminal = {
          success = "stopped";
          failure = "failed";
        };
      };
      clean = {
        operationId = "service.synthetic.clean";
        terminal = {
          success = "cleaned";
          failure = "failed";
        };
      };
    };
    endpoint = {
      endpointId = "synthetic-tcp";
    };
    stateRefs = [ "slot" ];
    logRefs = [ "service.synthetic" ];
  };

  nixfied.tasks.smoke = {
    operationId = "task.smoke.run";
    invocation = {
      tools = [ "synthetic-helper" ];
      run = [
        "nixfied-synthetic-helper"
        "task"
        "--host"
        "127.0.0.1"
        "--port"
        "\${port}"
      ];
    };
    requires = [ "synthetic" ];
    logRefs = [ "task.smoke" ];
    summaryRefs = [ "summary" ];
  };
}
