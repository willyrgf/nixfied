{
  pkgs,
  model,
  registry,
}:
let
  executor = import ../../nixfied/framework/runtime/executor.nix {
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
  export NIXFIED_RUNTIME_DIR_SCOPE_OVERRIDE="$TMPDIR/runtime-scope"
  mkdir -p "$REGISTRY_ROOT"
  mkdir -p "$NIXFIED_RUNTIME_DIR_SCOPE_OVERRIDE"

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

  if ! [ -f "$target/nixfied/framework/core/default.nix" ]; then
    echo "missing vendored framework/core/default.nix"
    exit 1
  fi

  if ! [ -f "$target/nixfied/project/module.nix" ]; then
    echo "missing vendored project/module.nix"
    exit 1
  fi

  if [ -f "$target/nixfied/.framework/.workspace" ]; then
    echo "vendored wrapper must not contain nixfied/.framework/.workspace marker"
    exit 1
  fi

  if [ -e "$target/nixfied/.framework" ]; then
    echo "vendored wrapper must not contain legacy nixfied/.framework path"
    exit 1
  fi

  if [ -f "$target/.workspace" ]; then
    echo "vendored wrapper must not contain repo-root .workspace marker"
    exit 1
  fi

  if ${pkgs.gnugrep}/bin/grep -Fq 'nixfied.url = "path:' "$target/flake.nix"; then
    echo "vendored wrapper should not require nixfied flake input"
    cat "$target/flake.nix"
    exit 1
  fi

  if ! ${pkgs.gnugrep}/bin/grep -Fq 'nixfiedLib = import ./nixfied/framework/core/default.nix {' "$target/flake.nix"; then
    echo "vendored wrapper should import ./nixfied/framework/core/default.nix"
    cat "$target/flake.nix"
    exit 1
  fi

  if ! ${pkgs.gnugrep}/bin/grep -Fq 'frameworkSourceRevision = import ./nixfied/framework/core/framework-revision.nix {' "$target/flake.nix"; then
    echo "vendored wrapper should import ./nixfied/framework/core/framework-revision.nix"
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
