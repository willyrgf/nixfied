# The derivation spec's golden vectors (docs/DERIVATION_SPEC.md §6) as eval
# fixtures on the Nix side. The runtime carries the same vectors as cargo
# fixtures; a divergence between either implementation and the spec text is a
# bug in that implementation (DERIVE-1).
{ pkgs, lib }:
let
  deriveFacts = import ../lib/derive-facts.nix { inherit lib; };

  leaf = requires: {
    kind = "leaf";
    inherit requires;
  };

  # V1-V3 — flattening and step paths.
  v1Tasks = {
    fmt = leaf [ ];
    clippy = leaf [ ];
    tests = leaf [ ];
    check = {
      kind = "composite";
      steps = {
        fmt.task = "fmt";
        clippy = {
          task = "clippy";
          dependsOn = [ "fmt" ];
        };
      };
    };
    ci = {
      kind = "composite";
      steps = {
        check.task = "check";
        tests = {
          task = "tests";
          dependsOn = [ "check" ];
        };
      };
    };
  };
  v2Tasks = {
    unit = leaf [ ];
    twice = {
      kind = "composite";
      steps = {
        again = {
          task = "unit";
          dependsOn = [ "first" ];
        };
        first.task = "unit";
      };
    };
  };

  # V4 — servicesRequired: union, connectsTo closure, canonical order.
  v4Tasks = {
    e2e = leaf [ "worker" ];
    smoke = leaf [ "api" ];
    lint = leaf [ ];
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

  # V8 — diamond dedup.
  v8Sr = deriveFacts.servicesRequired {
    tasks.e2e = leaf [ "a" "b" ];
    services = {
      db.connectsTo = [ ];
      a.connectsTo = [ "db" ];
      b.connectsTo = [ "db" ];
    };
  };

  # V9 — prepare task may be composite.
  v9Tasks = {
    migrate = leaf [ "dep" ];
    seed = leaf [ ];
    prep = {
      kind = "composite";
      steps = {
        migrate.task = "migrate";
        seed = {
          task = "seed";
          dependsOn = [ "migrate" ];
        };
      };
    };
    run = leaf [ "svc" ];
  };
  v9Sr = deriveFacts.servicesRequired {
    tasks = v9Tasks;
    services = {
      dep.connectsTo = [ ];
      svc.connectsTo = [ ];
    };
    prepareTaskOf = name: if name == "svc" then "prep" else null;
  };

  # V10 — connectsTo closure reaches a fixpoint beyond one hop.
  v10Sr = deriveFacts.servicesRequired {
    tasks.e2e = leaf [ "api" ];
    services = {
      api.connectsTo = [ "worker" ];
      worker.connectsTo = [ "db" ];
      db.connectsTo = [ "cache" ];
      cache.connectsTo = [ ];
    };
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
      name = "V1 flatten(ci)";
      ok = deriveFacts.flattenPlan v1Tasks "ci" == [
        {
          stepPath = "ci.check.fmt";
          leaf = "fmt";
          dependsOn = [ ];
        }
        {
          stepPath = "ci.check.clippy";
          leaf = "clippy";
          dependsOn = [ "ci.check.fmt" ];
        }
        {
          stepPath = "ci.tests";
          leaf = "tests";
          dependsOn = [
            "ci.check.clippy"
            "ci.check.fmt"
          ];
        }
      ];
    }
    {
      name = "V2 flatten(twice)";
      ok = deriveFacts.flattenPlan v2Tasks "twice" == [
        {
          stepPath = "twice.first";
          leaf = "unit";
          dependsOn = [ ];
        }
        {
          stepPath = "twice.again";
          leaf = "unit";
          dependsOn = [ "twice.first" ];
        }
      ];
    }
    {
      name = "V3 flatten(fmt)";
      ok = deriveFacts.flattenPlan v1Tasks "fmt" == [
        {
          stepPath = "fmt";
          leaf = "fmt";
          dependsOn = [ ];
        }
      ];
    }
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
    {
      name = "V8 diamond servicesRequired(e2e)";
      ok = v8Sr "e2e" == [ "a" "b" "db" ];
    }
    {
      name = "V9 composite prepare servicesRequired(run)";
      ok = v9Sr "run" == [ "dep" "svc" ];
    }
    {
      name = "V10 long connectsTo servicesRequired(e2e)";
      ok = v10Sr "e2e" == [ "api" "cache" "db" "worker" ];
    }
  ];
  failed = builtins.filter (vector: !vector.ok) vectors;
in
assert lib.assertMsg (failed == [ ])
  "derivation-spec golden vectors failed: ${builtins.concatStringsSep ", " (map (vector: vector.name) failed)}";
pkgs.runCommand "derive-facts-vectors" { } "touch $out"
