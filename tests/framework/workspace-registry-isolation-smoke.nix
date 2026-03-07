{
  pkgs,
  registry,
}:
let
  frameworkLib = import ../../nixfied/lib {
    inherit pkgs;
    system = pkgs.system;
  };

  workspaceA = builtins.path {
    path = ../..;
    name = "nixfied-workspace-a";
  };

  workspaceB = builtins.path {
    path = ../..;
    name = "nixfied-workspace-b";
  };

  compiledA = frameworkLib.mkNixfied {
    projectRoot = workspaceA;
    projectModules = [ ../../nixfied/project/module.nix ];
    extraModules = [ ];
    localOverrides = [ ];
  };

  compiledB = frameworkLib.mkNixfied {
    projectRoot = workspaceB;
    projectModules = [ ../../nixfied/project/module.nix ];
    extraModules = [ ];
    localOverrides = [ ];
  };

  harnessA = import ./lib/harness.nix {
    inherit
      pkgs
      registry
      ;
    model = compiledA.model;
    projectRoot = workspaceA;
  };
in
assert compiledA.model.identity.projectId == compiledB.model.identity.projectId;
assert compiledA.model.state.registry.root != compiledB.model.state.registry.root;
pkgs.runCommand "workspace-registry-isolation-smoke" { } ''
  set -euo pipefail
  ${harnessA.shellPrelude}

  ORCH_A="${harnessA.orchestrator}/bin/nixfied-orchestrator"
  ROOT_A="$TMPDIR/${compiledA.model.state.workspaceId}/registry"
  ROOT_B="$TMPDIR/${compiledB.model.state.workspaceId}/registry"
  mkdir -p "$ROOT_A" "$ROOT_B"

  run_count() {
    local root="$1"
    if [ ! -d "$root/orchestrator/runs" ]; then
      printf '0'
      return 0
    fi
    ${pkgs.findutils}/bin/find "$root/orchestrator/runs" -type f -name '*.json' | ${pkgs.coreutils}/bin/wc -l | ${pkgs.coreutils}/bin/tr -d '[:space:]'
  }

  before_a="$(run_count "$ROOT_A")"
  before_b="$(run_count "$ROOT_B")"

  REGISTRY_ROOT="$ROOT_A" "$ORCH_A" run-workflow workflow.ci.basic --summary > "$TMPDIR/workspace-a.out" 2>&1

  after_a="$(run_count "$ROOT_A")"
  after_b="$(run_count "$ROOT_B")"

  if [ "$after_a" -le "$before_a" ]; then
    fail "expected workspace A run registry to gain a run record"
  fi

  if [ "$after_b" != "$before_b" ]; then
    fail "workspace B registry changed after running workspace A"
  fi

  echo "OK: workspace-scoped registry roots isolate worktrees" > "$out"
''
