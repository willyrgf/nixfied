{
  pkgs,
  registry,
}:
let
  inherit (pkgs) lib;
  frameworkLib = import ../../nixfied/framework/core {
    inherit pkgs;
    inherit (pkgs) system;
  };

  sourceRoot = ../..;

  mkWorkspaceModule = workspaceId: {
    nixfied = {
      state.policy = {
        workspace.mode = lib.mkForce "literal";
        workspace.value = lib.mkForce workspaceId;
      };
    };
  };

  workspaceAModule = mkWorkspaceModule "workspace-a";
  workspaceBModule = mkWorkspaceModule "workspace-b";

  compiledA = frameworkLib.mkNixfied {
    projectRoot = sourceRoot;
    projectModules = [ ../../nixfied/project/module.nix ];
    extraModules = [ workspaceAModule ];
    localOverrides = [ ];
  };

  compiledB = frameworkLib.mkNixfied {
    projectRoot = sourceRoot;
    projectModules = [ ../../nixfied/project/module.nix ];
    extraModules = [ workspaceBModule ];
    localOverrides = [ ];
  };

  probeModelA = import ./lib/ci-probe-model.nix {
    inherit pkgs;
    inherit (compiledA) model;
  };

  harnessA = import ./lib/harness.nix {
    inherit
      pkgs
      registry
      ;
    model = probeModelA;
    inherit (compiledA) services;
    inherit (compiledA) serviceDefinitions;
    projectRoot = sourceRoot;
  };
in
assert compiledA.model.identity.projectId == compiledB.model.identity.projectId;
assert compiledA.model.state.policy.registryRoot != compiledB.model.state.policy.registryRoot;
pkgs.runCommand "workspace-registry-isolation-smoke" { } ''
  set -euo pipefail
  ${harnessA.shellPrelude}

  ORCH_A="${harnessA.orchestrator}/bin/nixfied-orchestrator"
  ROOT_A="$TMPDIR/${compiledA.model.state.policy.workspaceId}/registry"
  ROOT_B="$TMPDIR/${compiledB.model.state.policy.workspaceId}/registry"
  export CI_ARTIFACTS_ROOT="$TMPDIR/artifacts"
  mkdir -p "$ROOT_A" "$ROOT_B" "$CI_ARTIFACTS_ROOT"

  repo="$TMPDIR/repo"
  mkdir -p "$repo/subdir"
  printf 'tracked workspace registry probe\n' > "$repo/tracked.txt"
  printf 'tracked subdir probe\n' > "$repo/subdir/probe.txt"
  ${pkgs.git}/bin/git -C "$repo" init >/dev/null 2>&1
  ${pkgs.git}/bin/git -C "$repo" add tracked.txt subdir/probe.txt
  ${pkgs.git}/bin/git -C "$repo" -c user.name=nixfied -c user.email=nixfied@example.invalid \
    commit -m "init workspace registry probe repo" >/dev/null 2>&1

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

  REGISTRY_ROOT="$ROOT_A" NIXFIED_CALLER_PWD="$repo/subdir" \
    "$ORCH_A" run-workflow workflow.ci.basic --summary > "$TMPDIR/workspace-a.out" 2>&1

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
