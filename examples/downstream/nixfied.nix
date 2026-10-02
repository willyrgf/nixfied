# Downstream worked example: a realistic small system.
#
# This is the copy-paste adoption guide other teams start from. It composes a
# Postgres database (via the reusable `adapters.postgres` Nix-side adapter) with
# two first-party services written as plain Nix-built executables - an `api` and
# a `worker` - and orchestrates them with a `release` composite task. Everything below
# compiles into the generic manifest primitives; the runtime gains no knowledge of
# Postgres, the api, or the worker.
#
# Build it:    nix build --no-write-lock-file ./examples/downstream#manifest
# Run it:      nixfied-runtime run --manifest <store>/manifest.json
{ pkgs, adapters, ... }:
let
  # Static HTTP responses keep the example focused on service composition.
  responses = pkgs.linkFarm "downstream-responses" (
    map
      (label: {
        name = label;
        path = pkgs.writeText "response" "${label}-ok\n";
      })
      [
        "api"
        "worker"
      ]
  );
  appHelper = pkgs.writeShellApplication {
    name = "downstream-app";
    runtimeInputs = [
      pkgs.darkhttpd
      pkgs.curl
      pkgs.postgresql
    ];
    text = ''
      if [[ $# -lt 7 || "$2" != --label || "$4" != --host || "$6" != --port ]]; then
        echo "expected service|task --label LABEL --host HOST --port PORT" >&2
        exit 2
      fi
      mode="$1"; label="$3"; host="$5"; port="$7"
      shift 7
      case "$label" in api|worker) ;; *) exit 2 ;; esac
      check_http() {
        local reply
        reply=$(curl -q --silent --show-error --fail --noproxy '*' --globoff \
          --max-time 5 --max-filesize 1024 --write-out . "http://$1/")
        [[ "$reply" == "$2-ok"$'\n.' ]] || { echo "invalid downstream response" >&2; exit 1; }
        printf '%s-ok\n' "$2"
      }
      case "$mode" in
        service)
          if [[ $# != 0 ]]; then
            [[ $# == 2 && "$1" == --upstream ]] || exit 2
            check_http "$2" api
          fi
          exec darkhttpd "${responses}/$label" --single-file --addr "$host" --port "$port" --no-keepalive
          ;;
        task)
          check_http "$host:$port" "$label"
          if [[ $# != 0 ]]; then
            [[ $# == 2 && "$1" == --also ]] || exit 2
            check_http "$2" worker
            # Real protocol success proves the environment DSN was substituted.
            [[ $(PGCONNECT_TIMEOUT=5 psql "$NIXFIED_DEMO_DSN" -X -w -tAc 'SELECT 1') == 1 ]]
          fi
          ;;
        *) exit 2 ;;
      esac
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
    # Operation ids and terminal tokens are derived (service.<name>.<op> and
    # the per-class defaults); only the start mechanism is authored.
    lifecycle = {
      start.invocation = {
        tools = [ "app" ];
        run = serviceRun name ++ extraStartArgs;
      };
    };
    endpoint = {
      endpointId = "${name}-tcp";
      readyProbe = {
        tools = [ "app" ];
        run = taskRun name;
      };
      healthProbe = {
        tools = [ "app" ];
        run = taskRun name;
      };
    };
    inherit connectsTo;
    logRefs = [ "service.${name}" ];
  };
in
{
  # Reuse the Postgres adapter for the database tier. It contributes the
  # `postgres` service and the `smoke-query` task (a `SELECT 1`);
  # the declarations below merge with those.
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
    invocation = {
      tools = [ "app" ];
      run = taskRun "api";
    };
    requires = [ "api" ];
    logRefs = [ "task.ping-api" ];
  };
  nixfied.tasks.ping-worker = {
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
    invocation = {
      tools = [ "app" ];
      run = taskRun "api" ++ [
        "--also"
        "\${host:worker}:\${port:worker}"
      ];
      env = {
        NIXFIED_DEMO_DSN = "postgresql://postgres@\${host:postgres}:\${port:postgres}/postgres";
      };
    };
    requires = [
      "api"
      "worker"
      "postgres"
    ];
    logRefs = [ "task.release-gate" ];
  };

  # The adopter-owned public surface: the release flow is the one verb this
  # example exports (`nix run .#release` once wired through projectApps).
  nixfied.surface.verbs.release = "Run the downstream release workflow";

  # A release flow: prove the database answers, then exercise both services,
  # then run the gate over the whole stack. A composite task; the services it
  # needs are derived from the referenced leaves.
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
