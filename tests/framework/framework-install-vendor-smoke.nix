{
  pkgs,
  model,
  registry,
}:
let
  executor = import ../../nixfied/runner/executor.nix {
    inherit
      pkgs
      model
      registry
      ;
    projectRoot = ../..;
  };
in
pkgs.runCommand "framework-install-vendor-smoke" { } ''
  set -euo pipefail

  EXECUTOR="${executor}/bin/nixfied-executor"
  export REGISTRY_ROOT="$TMPDIR/registry"
  mkdir -p "$REGISTRY_ROOT"

  target="$TMPDIR/vendor-wrapper"
  "$EXECUTOR" run-task task.framework.install --vendor --target "$target" > "$TMPDIR/install.out" 2>&1

  if ! [ -f "$target/flake.nix" ]; then
    echo "missing generated flake.nix"
    cat "$TMPDIR/install.out"
    exit 1
  fi

  if ! [ -f "$target/nixfied/lib/default.nix" ]; then
    echo "missing vendored lib/default.nix"
    exit 1
  fi

  if ! [ -f "$target/nixfied/project/module.nix" ]; then
    echo "missing vendored project/module.nix"
    exit 1
  fi

  if ${pkgs.gnugrep}/bin/grep -Fq 'nixfied.url = "path:' "$target/flake.nix"; then
    echo "vendored wrapper should not require nixfied flake input"
    cat "$target/flake.nix"
    exit 1
  fi

  if ! ${pkgs.gnugrep}/bin/grep -Fq 'nixfiedLib = import ./nixfied/lib/default.nix {' "$target/flake.nix"; then
    echo "vendored wrapper should import ./nixfied/lib/default.nix"
    cat "$target/flake.nix"
    exit 1
  fi

  if ! ${pkgs.gnugrep}/bin/grep -Fq 'projectModules = [ ./nixfied/project/module.nix ];' "$target/flake.nix"; then
    echo "vendored wrapper should use ./nixfied/project/module.nix"
    cat "$target/flake.nix"
    exit 1
  fi

  if ${pkgs.gnugrep}/bin/grep -Fq './nixfied/nixfied/project/module.nix' "$target/flake.nix"; then
    echo "vendored wrapper contains legacy nested project path"
    cat "$target/flake.nix"
    exit 1
  fi

  echo "OK: framework::install --vendor wrapper contract is validated" > "$out"
''
