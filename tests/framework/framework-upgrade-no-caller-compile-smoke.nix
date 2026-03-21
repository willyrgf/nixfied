{
  pkgs,
  model,
  services,
  registry,
}:
let
  frameworkLib = import ../../nixfied/framework/core {
    inherit pkgs;
    system = pkgs.system;
  };
  shellHelpers = import ./lib/shell-helpers.nix { inherit pkgs; };
  executor = import ../../nixfied/framework/runtime/executor.nix {
    inherit
      pkgs
      model
      services
      registry
      ;
    projectRoot = ../..;
  };

  frameworkOutputs = frameworkLib.mkFlakeOutputs {
    projectRoot = ../..;
    projectModules = [ ../../nixfied/project/module.nix ];
    extraModules = [ ];
    localOverrides = [ ];
  };
in
pkgs.runCommand "framework-upgrade-no-caller-compile-smoke" { } ''
  set -euo pipefail
  ${shellHelpers.shellPrelude}

  EXECUTOR="${executor}/bin/nixfied-executor"
  UPGRADE_APP="${frameworkOutputs.apps."framework::upgrade".program}"

  export REGISTRY_ROOT="$TMPDIR/registry"
  export NIXFIED_RUNTIME_DIR_SCOPE_OVERRIDE="$TMPDIR/runtime-scope"
  mkdir -p "$REGISTRY_ROOT" "$NIXFIED_RUNTIME_DIR_SCOPE_OVERRIDE"

  target_repo="$TMPDIR/upgrade-target"
  mkdir -p "$target_repo"

  "$EXECUTOR" run-task task.framework.install --vendor --target "$target_repo" > "$TMPDIR/bootstrap.out" 2>&1 || {
    cat "$TMPDIR/bootstrap.out"
    fail "bootstrap vendored wrapper failed"
  }

  cat > "$target_repo/nixfied/project/module.nix" <<'EOF'
{ }:
throw "caller project compiled unexpectedly during framework::upgrade"
EOF

  (
    cd "$target_repo"
    "$UPGRADE_APP" --target . > "$TMPDIR/upgrade.out" 2>&1
  ) || {
    cat "$TMPDIR/upgrade.out"
    fail "framework::upgrade should not compile the caller project"
  }

  require_contains "$TMPDIR/upgrade.out" "OK: vendored wrapper upgraded at ./flake.nix"
  require_contains "$target_repo/nixfied/project/module.nix" "caller project compiled unexpectedly during framework::upgrade"

  echo "OK: framework::upgrade can run from a poisoned caller repo without compiling it" > "$out"
''
