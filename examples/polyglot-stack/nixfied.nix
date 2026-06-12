# Polyglot example: two services written in different languages (Python and
# Perl) plus a dependent task per service, composed entirely through the generic
# model primitives. No new runtime capability is used.
{ pkgs, ... }:
let
  pythonService = pkgs.writeTextFile {
    name = "polyglot-python";
    destination = "/bin/polyglot-python";
    executable = true;
    text = ''
      #!${pkgs.python3}/bin/python3
      import argparse, socket, sys

      def serve(a):
          with socket.socket(socket.AF_INET, socket.SOCK_STREAM) as s:
              s.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1)
              s.bind((a.host, a.port)); s.listen()
              print(f"python service on {a.host}:{a.port}", flush=True)
              while True:
                  c, _ = s.accept()
                  with c:
                      c.recv(4096); c.sendall(b"python-ok\n")

      def ping(a):
          with socket.create_connection((a.host, a.port), timeout=5) as c:
              c.sendall(b"ping\n"); sys.stdout.write(c.recv(4096).decode())

      p = argparse.ArgumentParser()
      sub = p.add_subparsers(dest="cmd", required=True)
      for name in ("service", "task"):
          q = sub.add_parser(name); q.add_argument("--host", required=True); q.add_argument("--port", required=True, type=int)
      sub.add_parser("stop")
      a = p.parse_args()
      if a.cmd == "service": serve(a)
      elif a.cmd == "task": ping(a)
      else: sys.exit(0)
    '';
  };

  perlService = pkgs.writeTextFile {
    name = "polyglot-perl";
    destination = "/bin/polyglot-perl";
    executable = true;
    text = ''
      #!${pkgs.perl}/bin/perl
      use strict; use warnings; use IO::Socket::INET;
      my %o; my $cmd = shift @ARGV; $cmd = "" unless defined $cmd;
      while (@ARGV) { my $k = shift @ARGV; my $v = shift @ARGV; $v = "" unless defined $v; $k =~ s/^--//; $o{$k} = $v; }
      if ($cmd eq "service") {
        my $srv = IO::Socket::INET->new(LocalHost=>$o{host}, LocalPort=>$o{port}, Proto=>"tcp", Listen=>5, ReuseAddr=>1) or die "bind: $!";
        $| = 1; print "perl service on $o{host}:$o{port}\n";
        while (my $c = $srv->accept()) { my $b; $c->recv($b, 4096); $c->send("perl-ok\n"); close $c; }
      } elsif ($cmd eq "task") {
        my $c = IO::Socket::INET->new(PeerHost=>$o{host}, PeerPort=>$o{port}, Proto=>"tcp", Timeout=>5) or die "connect: $!";
        $c->send("ping\n"); my $b; $c->recv($b, 4096); print $b; close $c;
      } else { exit 0; }
    '';
  };

  serviceArgs = [
    "service"
    "--host"
    "127.0.0.1"
    "--port"
    "\${port}"
  ];
  taskArgs = [
    "task"
    "--host"
    "127.0.0.1"
    "--port"
    "\${port}"
  ];

  mkService = name: program: {
    lifecycle = {
      start.invocation = {
        tools = [ name ];
        run = [ program ] ++ serviceArgs;
      };
    };
    endpoint = {
      endpointId = "${name}-tcp";
    };
    logRefs = [ "service.${name}" ];
  };
in
{
  nixfied.project.projectId = "polyglot-stack";
  nixfied.project.name = "Polyglot Stack";
  nixfied.codebases.main.logicalRoot = ".";
  nixfied.placement.ports.base = 24780;

  nixfied.closures.api = {
    package = pythonService;
    executable = "bin/polyglot-python";
    effects = [
      "process"
      "network-listener"
    ];
  };
  nixfied.closures.worker = {
    package = perlService;
    executable = "bin/polyglot-perl";
    effects = [
      "process"
      "network-listener"
    ];
  };

  nixfied.services.api = mkService "api" "polyglot-python";
  nixfied.services.worker = mkService "worker" "polyglot-perl";

  nixfied.tasks.ping-api = {
    invocation = {
      tools = [ "api" ];
      run = [ "polyglot-python" ] ++ taskArgs;
    };
    requires = [ "api" ];
    logRefs = [ "task.ping-api" ];
  };
  nixfied.tasks.ping-worker = {
    invocation = {
      tools = [ "worker" ];
      run = [ "polyglot-perl" ] ++ taskArgs;
    };
    requires = [ "worker" ];
    logRefs = [ "task.ping-worker" ];
  };

  # The composed check the gate selects: both pings, each bringing up its own
  # service through the derived union.
  nixfied.tasks.all = {
    kind = "composite";
    steps = {
      ping-api.task = "ping-api";
      ping-worker.task = "ping-worker";
    };
  };
}
