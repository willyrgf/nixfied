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
pkgs.runCommand "framework-install-no-caller-compile-smoke" { } ''
  set -euo pipefail
  ${shellHelpers.shellPrelude}

  INSTALL_APP="${frameworkOutputs.apps."framework::install".program}"
  GIT_BIN="${pkgs.git}/bin/git"

  target_repo="$TMPDIR/install-target"
  mkdir -p "$target_repo/nixfied/project"
  "$GIT_BIN" -C "$target_repo" init -q
  echo '{ description = "poisoned target"; }' > "$target_repo/flake.nix"
  cat > "$target_repo/nixfied/project/module.nix" <<'EOF'
{ }:
throw "caller project compiled unexpectedly during framework::install"
EOF

  (
    cd "$target_repo"
    "$INSTALL_APP" --vendor > "$TMPDIR/install.out" 2>&1
  ) || {
    cat "$TMPDIR/install.out"
    fail "framework::install should not compile the caller project"
  }

  require_contains "$TMPDIR/install.out" "OK: vendored wrapper upgraded at ./flake.nix"
  require_file "$target_repo/nixfied/framework/core/default.nix"
  require_contains "$target_repo/nixfied/project/module.nix" "caller project compiled unexpectedly during framework::install"

  echo "OK: framework::install can run from a poisoned caller repo without compiling it" > "$out"
''
