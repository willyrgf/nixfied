{ pkgs, target }:
let
  helper = pkgs.writeTextFile {
    name = "nixfied-m0-helper";
    destination = "/bin/nixfied-m0-helper";
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
                  f"nixfied-m0-helper listening on {args.host}:{args.port}",
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


      parser = argparse.ArgumentParser(prog="nixfied-m0-helper")
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
  package = helper;
  spec = {
    closureId = "m0-helper";
    kind = "executable";
    storePath = "${helper}";
    executable = "${helper}/bin/nixfied-m0-helper";
    targetSystem = target.closureSystem;
    operationBindings = [
      "service.synthetic.start"
      "service.synthetic.stop"
      "task.smoke.run"
    ];
    requiresExecutable = true;
    effects = [
      "process"
      "network-listener"
    ];
  };
}
