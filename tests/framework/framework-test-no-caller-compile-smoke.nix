{ pkgs }:
let
  frameworkLib = import ../../nixfied/framework/core {
    inherit pkgs;
    system = pkgs.system;
  };
  shellHelpers = import ./lib/shell-helpers.nix { inherit pkgs; };

  frameworkOutputs = frameworkLib.mkFlakeOutputs {
    projectRoot = ../..;
    projectModules = [ ../../nixfied/project/module.nix ];
    extraModules = [ ];
    localOverrides = [ ];
  };
in
pkgs.runCommand "framework-test-no-caller-compile-smoke" { } ''
  set -euo pipefail
  ${shellHelpers.shellPrelude}

  FRAMEWORK_TEST_APP="${frameworkOutputs.apps."framework::test".program}"
  GIT_BIN="${pkgs.git}/bin/git"

  export REGISTRY_ROOT="$TMPDIR/registry"
  export NIXFIED_RUNTIME_DIR_SCOPE_OVERRIDE="$TMPDIR/runtime-scope"
  mkdir -p "$REGISTRY_ROOT" "$NIXFIED_RUNTIME_DIR_SCOPE_OVERRIDE"

  target_repo="$TMPDIR/framework-test-target"
  mkdir -p "$target_repo/nixfied/project"
  "$GIT_BIN" -C "$target_repo" init -q
  echo '{ description = "poisoned target"; }' > "$target_repo/flake.nix"
  cat > "$target_repo/nixfied/project/module.nix" <<'EOF'
{ }:
throw "caller project compiled unexpectedly during framework::test"
EOF

  (
    cd "$target_repo"
    "$FRAMEWORK_TEST_APP" --list-shards > "$TMPDIR/framework-test.out" 2>&1
  ) || {
    cat "$TMPDIR/framework-test.out"
    fail "framework::test should not compile the caller project"
  }

  require_contains "$TMPDIR/framework-test.out" "flake-check"
  require_contains "$TMPDIR/framework-test.out" "self-host"
  require_contains "$target_repo/nixfied/project/module.nix" "caller project compiled unexpectedly during framework::test"

  echo "OK: framework::test can run from a poisoned caller repo without compiling it" > "$out"
''
