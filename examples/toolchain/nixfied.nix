# The toolchain-shaped standing example: the adopter shape the first external
# adoption proved the old vocabulary could not express, kept provably
# expressible forever after (the methodology fix from PROBLEM_COMPOSITION.md).
#
# It demonstrates, in one model the gate runs like every other example:
#   - leaves with a real multi-tool PATH (plain packages synthesized into tool
#     closures; the runtime only assembles PATH from their roots);
#   - heterogeneous per-leaf `requires` (a lint that needs nothing next to a
#     db test that needs postgres next to an e2e that needs the worker);
#   - nested composites (`ci` references `check`), with `lib.seq` sugar;
#   - an **endpoint-less worker service** (durable is not listening): owned,
#     invocation-probed, contained, and cleaned, `connectsTo` postgres for
#     addressability — but binding no socket and claiming no port;
#   - one deliberately retry-shaped leaf documenting where STATIC-1's
#     boundary is and what the sanctioned escape looks like.
{
  pkgs,
  adapters,
  nixfiedLib,
  ...
}:
let
  # A worker daemon with no listener: it connects OUT to postgres and signals
  # liveness through a heartbeat file under the slot state root — exactly the
  # queue-consumer/indexer shape the endpoint requirement used to force below
  # the seam.
  workerDaemon = pkgs.writeShellApplication {
    name = "toolchain-worker";
    runtimeInputs = [ pkgs.coreutils ];
    text = ''
      state_dir="''${1:?missing state dir}"
      db_url="''${2:?missing db url}"
      case "$db_url" in
        *'%{'*) echo "unsubstituted placeholder in db url: $db_url" >&2; exit 3 ;;
      esac
      while true; do
        date +%s > "$state_dir/worker-heartbeat"
        sleep 1
      done
    '';
  };
in
{
  imports = [ adapters.postgres ];

  nixfied.project.projectId = "toolchain";
  nixfied.project.name = "Toolchain Example";
  nixfied.codebases.main.logicalRoot = ".";
  nixfied.placement.ports.base = 26280;

  # ---- the endpoint-less worker -------------------------------------------
  nixfied.closures.worker = {
    package = workerDaemon;
    executable = "bin/toolchain-worker";
    # No `network-listener`: the worker binds nothing (effects coherence
    # rejects the attestation on an endpoint-less service).
    effects = [
      "process"
      "file-write"
    ];
  };

  nixfied.services.worker = {
    # No `endpoint`/`endpoints`: durable, not listening.
    lifecycle = {
      start.invocation = {
        tools = [ "worker" ];
        run = [
          "toolchain-worker"
          "\${stateDir}"
          # connectsTo makes postgres addressable from here; the script fails
          # on an unsubstituted placeholder, so the gate proves substitution.
          "postgresql://postgres@\${host:postgres}:\${port:postgres}/postgres"
        ];
      };
      # Readiness means "the probe answers": the heartbeat file exists.
      ready.probe = {
        kind = "exec";
        invocation = {
          tools = [ pkgs.bash ];
          run = [
            "bash"
            "-c"
            ''test -e "$1"''
            "probe"
            "\${stateDir}/worker-heartbeat"
          ];
        };
        timeoutMs = 1000;
        retryIntervalMs = 200;
        maxAttempts = 30;
      };
      health.probe = {
        kind = "exec";
        invocation = {
          tools = [ pkgs.bash ];
          run = [
            "bash"
            "-c"
            ''test -e "$1"''
            "probe"
            "\${stateDir}/worker-heartbeat"
          ];
        };
        timeoutMs = 1000;
        retryIntervalMs = 200;
        maxAttempts = 30;
      };
    };
    connectsTo = [ "postgres" ];
    logRefs = [ "service.worker" ];
  };

  # ---- leaves with heterogeneous requirements ------------------------------
  # A multi-tool PATH from plain packages: the compiler synthesizes a tool
  # closure per package; the runtime assembles PATH from their bin roots and
  # the child sees nothing else (hermetic env).
  nixfied.tasks.lint = {
    invocation = {
      tools = [
        pkgs.bash
        pkgs.gnugrep
        pkgs.coreutils
      ];
      run = [
        "bash"
        "-c"
        # Children resolve grep/wc through the assembled PATH.
        "printf 'lint target\\n' | grep -c target | grep -qx 1"
      ];
    };
  };

  nixfied.tasks.unit = {
    invocation = {
      tools = [
        pkgs.bash
        pkgs.coreutils
      ];
      run = [
        "bash"
        "-c"
        "test \"$(seq 3 | tail -n1)\" = 3"
      ];
    };
  };

  # STATIC-1's boundary, demonstrated: composites are static DAGs — no
  # retries, conditionals, or loops in the contract. A flaky operation's
  # sanctioned escape is dynamism INSIDE the opaque leaf: this leaf retries
  # its own work and presents one bounded exit to the model.
  nixfied.tasks.flaky-probe = {
    invocation = {
      tools = [
        pkgs.bash
        pkgs.coreutils
      ];
      run = [
        "bash"
        "-c"
        ''
          for attempt in 1 2 3; do
            marker="$1/flaky-attempt"
            if [ -e "$marker" ]; then exit 0; fi
            touch "$marker"
          done
          exit 1
        ''
        "flaky"
        "\${stateDir}"
      ];
    };
  };

  # A db check that needs postgres ready while it runs (and nothing else).
  nixfied.tasks.db-test = {
    invocation = {
      tools = [ "pg-psql" ];
      run = [
        "psql"
        "-h"
        "127.0.0.1"
        "-p"
        "\${port:postgres}"
        "-U"
        "postgres"
        "-d"
        "postgres"
        "-w"
        "-tAc"
        "SELECT 41 + 1"
      ];
    };
    requires = [ "postgres" ];
  };

  # An e2e check that needs the (endpoint-less) worker alive while it runs:
  # `requires` toward an endpoint-less service keeps its ready-ordering and
  # derivation meaning — there is just nothing to address.
  nixfied.tasks.e2e = {
    invocation = {
      tools = [
        pkgs.bash
        pkgs.coreutils
      ];
      run = [
        "bash"
        "-c"
        ''test -e "$1/worker-heartbeat"''
        "e2e"
        "\${stateDir}"
      ];
    };
    requires = [ "worker" ];
  };

  # ---- composites -----------------------------------------------------------
  # `lib.seq` compiles to a dependsOn chain; `ci` nests `check` (run-once is
  # per step — natural authoring produces no duplicates).
  nixfied.tasks.check = {
    kind = "composite";
    steps = nixfiedLib.seq [
      "lint"
      "unit"
      "flaky-probe"
    ];
  };

  nixfied.tasks.ci = {
    kind = "composite";
    steps = {
      check.task = "check";
      db-test = {
        task = "db-test";
        dependsOn = [ "check" ];
      };
      e2e = {
        task = "e2e";
        dependsOn = [ "db-test" ];
      };
    };
  };

  # The adopter-owned public surface.
  nixfied.surface.verbs = [
    "check"
    "ci"
  ];
}
