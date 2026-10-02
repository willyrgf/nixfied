# Synthetic service adapter.
#
# A Nix-side adapter is a library that compiles a concrete service into the
# generic manifest primitives. This one provides a self-contained foreground TCP
# service plus a dependent smoke task, used by the minimal example and proofs.
# The runtime gains no knowledge of it; everything here is generic data.
{ pkgs, ... }:
let
  response = pkgs.writeText "synthetic-response" "ok";
  helper = pkgs.writeShellApplication {
    name = "nixfied-synthetic-helper";
    runtimeInputs = [
      pkgs.darkhttpd
      pkgs.curl
    ];
    text = ''
      if [[ $# != 5 || "$2" != --host || "$4" != --port ]]; then
        echo "expected service|task --host HOST --port PORT" >&2
        exit 2
      fi
      case "$1" in
        service)
          exec darkhttpd ${response} --single-file --addr "$3" --port "$5" --no-keepalive
          ;;
        task)
          reply=$(curl -q --silent --show-error --fail --noproxy '*' --globoff \
            --max-time 5 --max-filesize 1024 --write-out . "http://$3:$5/")
          [[ "$reply" == ok. ]] || { echo "invalid synthetic protocol response" >&2; exit 1; }
          printf 'ok\n'
          ;;
        *) echo "unknown synthetic command" >&2; exit 2 ;;
      esac
    '';
  };
  probe = {
    tools = [ "synthetic-helper" ];
    run = [
      "nixfied-synthetic-helper"
      "task"
      "--host"
      "\${host}"
      "--port"
      "\${port}"
    ];
  };
in
{
  nixfied.closures.synthetic-helper = {
    package = helper;
    executable = "bin/nixfied-synthetic-helper";
    kind = "executable";
    requiresExecutable = true;
    effects = [
      "process"
      "network-listener"
    ];
  };

  nixfied.services.synthetic = {
    lifecycle = {
      start = {
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
      };
    };
    endpoint = {
      endpointId = "synthetic-tcp";
      readyProbe = probe;
      healthProbe = probe;
    };
    stateRefs = [ "slot" ];
    logRefs = [ "service.synthetic" ];
  };

  nixfied.tasks.smoke = {
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
