{ pkgs, model }:
let
  lib = pkgs.lib;
  compiler = import ../../nixfied/compiler {
    inherit pkgs;
    canonical = import ../../nixfied/framework/core/canonical.nix { inherit lib; };
    modules = import ../../nixfied/modules;
    system = pkgs.system;
    projectRoot = ../..;
    frameworkSourceRevision = import ../../nixfied/framework/core/framework-revision.nix {
      sourcePath = ../../.;
      metadataPath = ../../VENDORED.txt;
    };
  };

  poisonModule =
    { lib, ... }:
    {
      nixfied.services.nginx.enable = lib.mkForce true;
      nixfied.services.nginx.probes.health = {
        strategy = "replace";
        steps = [
          {
            kind = "exec";
            command = throw "nginx health probe normalized unexpectedly";
          }
        ];
      };
      nixfied.services.nginx.probes.ready = {
        strategy = "replace";
        steps = [
          {
            kind = "exec";
            command = throw "nginx readiness probe normalized unexpectedly";
          }
        ];
      };
    };

  poisoned = compiler.compileCore {
    projectModules = [ ../../nixfied/project/module.nix ];
    extraModules = [ poisonModule ];
    localOverrides = [ ];
  };

  validateCommand = model.tasks."task.ops.validate-env".runner.command;
  portsCommand = model.tasks."task.ops.ports".runner.command;
  checkPortsCommand = model.tasks."task.ops.check-ports".runner.command;
  healthCommand = model.tasks."task.ops.health".runner.command;
  readyCommand = model.tasks."task.ops.ready".runner.command;
  isolationCommand = model.tasks."task.ops.test-isolation".runner.command;

  poisonedValidateCommand = poisoned.tasks."task.ops.validate-env".runner.command;
  poisonedPortsCommand = poisoned.tasks."task.ops.ports".runner.command;
  poisonedCheckPortsCommand = poisoned.tasks."task.ops.check-ports".runner.command;
  poisonedIsolationCommand = poisoned.tasks."task.ops.test-isolation".runner.command;
in
assert builtins.hasAttr "task.ops.validate-env" model.tasks;
assert builtins.hasAttr "task.ops.ports" model.tasks;
assert builtins.hasAttr "task.ops.check-ports" model.tasks;
assert builtins.hasAttr "task.ops.health" model.tasks;
assert builtins.hasAttr "task.ops.ready" model.tasks;
assert builtins.hasAttr "task.ops.test-isolation" model.tasks;
assert pkgs.lib.hasInfix "OK: environment is valid" validateCommand;
assert pkgs.lib.hasInfix "INFO: Port assignments" portsCommand;
assert pkgs.lib.hasInfix "INFO: Port status" checkPortsCommand;
assert pkgs.lib.hasInfix "OK: health checks passed" healthCommand;
assert pkgs.lib.hasInfix "OK: readiness checks passed" readyCommand;
assert pkgs.lib.hasInfix "isolation cell start" isolationCommand;
assert pkgs.lib.hasInfix "isolation_slots=()" isolationCommand;
assert pkgs.lib.hasInfix "run_env_entries=()" isolationCommand;
assert (!pkgs.lib.hasInfix "/bin/jq" isolationCommand);
assert builtins.deepSeq poisonedValidateCommand true;
assert builtins.deepSeq poisonedPortsCommand true;
assert builtins.deepSeq poisonedCheckPortsCommand true;
assert builtins.deepSeq poisonedIsolationCommand true;
pkgs.runCommand "operations-contract" { } ''
  cat > task-ops-health.sh <<'EOF'
  ${healthCommand}
  EOF
  cat > task-ops-ready.sh <<'EOF'
  ${readyCommand}
  EOF
  cat > task-ops-validate-env.sh <<'EOF'
  ${validateCommand}
  EOF
  cat > task-ops-ports.sh <<'EOF'
  ${portsCommand}
  EOF
  cat > task-ops-check-ports.sh <<'EOF'
  ${checkPortsCommand}
  EOF
  cat > task-ops-test-isolation.sh <<'EOF'
  ${isolationCommand}
  EOF
  ${pkgs.bash}/bin/bash -n task-ops-health.sh
  ${pkgs.bash}/bin/bash -n task-ops-ready.sh
  ${pkgs.bash}/bin/bash -n task-ops-validate-env.sh
  ${pkgs.bash}/bin/bash -n task-ops-ports.sh
  ${pkgs.bash}/bin/bash -n task-ops-check-ports.sh
  ${pkgs.bash}/bin/bash -n task-ops-test-isolation.sh
  echo "OK: operations health/readiness and cheap ops contracts are stable" > "$out"
''
