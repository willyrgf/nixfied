# The derivation spec's golden vectors (docs/DERIVATION_SPEC.md §6) as eval
# fixtures on the Nix side. The runtime carries the same vectors as cargo
# fixtures; a divergence between either implementation and the spec text is a
# bug in that implementation (DERIVE-1).
{ pkgs, lib }:
let
  deriveFacts = import ../lib/derive-facts.nix { inherit lib; };

  # V4 — servicesRequired: union, connectsTo closure, canonical order.
  v4Tasks = {
    e2e = {
      kind = "leaf";
      requires = [ "worker" ];
    };
    smoke = {
      kind = "leaf";
      requires = [ "api" ];
    };
    lint = {
      kind = "leaf";
      requires = [ ];
    };
    all = {
      kind = "composite";
      steps = {
        e2e.task = "e2e";
        lint.task = "lint";
        smoke.task = "smoke";
      };
    };
  };
  v4Services = {
    postgres.connectsTo = [ ];
    api.connectsTo = [ "postgres" ];
    worker.connectsTo = [ "postgres" ];
  };
  sr = deriveFacts.servicesRequired {
    tasks = v4Tasks;
    services = v4Services;
  };

  # V5/V7 — operationBindings: run[0] closure binds, tools do not; one closure
  # dispatched by several leaves sorts its bindings.
  v5Basenames = {
    cargoC = "cargo";
    gitC = "git";
    psqlC = "psql";
    pg-serverC = "postgres";
  };
  v5Positions = [
    {
      operationId = "task.build.run";
      toolIds = [
        "cargoC"
        "gitC"
      ];
      program = "cargo";
    }
    {
      operationId = "task.query.run";
      toolIds = [ "psqlC" ];
      program = "psql";
    }
    {
      operationId = "service.postgres.start";
      toolIds = [ "pg-serverC" ];
      program = "postgres";
    }
    {
      operationId = "task.fmt.run";
      toolIds = [ "cargoC" ];
      program = "cargo";
    }
  ];
  bindings = deriveFacts.operationBindings {
    positions = v5Positions;
    executableBasenames = v5Basenames;
  };

  vectors = [
    {
      name = "V4 servicesRequired(all)";
      ok = sr "all" == [ "api" "postgres" "worker" ];
    }
    {
      name = "V4 servicesRequired(e2e)";
      ok = sr "e2e" == [ "postgres" "worker" ];
    }
    {
      name = "V4 servicesRequired(lint)";
      ok = sr "lint" == [ ];
    }
    {
      name = "V4 servicesRequired(smoke)";
      ok = sr "smoke" == [ "api" "postgres" ];
    }
    {
      name = "V5/V7 bindings(cargoC) sorted";
      ok =
        bindings "cargoC" == [
          "task.build.run"
          "task.fmt.run"
        ];
    }
    {
      name = "V5 bindings(gitC) tool-set member binds nothing";
      ok = bindings "gitC" == [ ];
    }
    {
      name = "V5 bindings(psqlC)";
      ok = bindings "psqlC" == [ "task.query.run" ];
    }
    {
      name = "V5 bindings(pg-serverC)";
      ok = bindings "pg-serverC" == [ "service.postgres.start" ];
    }
    {
      name = "V6 default leaf operation id";
      ok = deriveFacts.leafOperationId "fmt" == "task.fmt.run";
    }
    {
      name = "V6 default service operation id";
      ok = deriveFacts.serviceOperationId "postgres" "ready" == "service.postgres.ready";
    }
    {
      name = "V6 default ready terminal";
      ok = deriveFacts.terminalDefaults.ready == {
        success = "ready";
        failure = "not-ready";
      };
    }
  ];
  failed = builtins.filter (vector: !vector.ok) vectors;
in
assert lib.assertMsg (failed == [ ])
  "derivation-spec golden vectors failed: ${builtins.concatStringsSep ", " (map (vector: vector.name) failed)}";
pkgs.runCommand "derive-facts-vectors" { } "touch $out"
