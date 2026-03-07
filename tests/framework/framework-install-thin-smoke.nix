{
  pkgs,
  model,
  registry,
}:
let
  harness = import ./lib/harness.nix {
    inherit
      pkgs
      model
      registry
      ;
    projectRoot = ../..;
  };
in
pkgs.runCommand "framework-install-thin-smoke" { } ''
  set -euo pipefail
  ${harness.shellPrelude}

  ORCH="${harness.orchestrator}/bin/nixfied-orchestrator"
  export REGISTRY_ROOT="$TMPDIR/registry"
  mkdir -p "$REGISTRY_ROOT"

  target="$TMPDIR/thin-wrapper"
  "$ORCH" run-task task.framework.install --target "$target" > "$TMPDIR/thin-install.out" 2>&1

  require_file "$target/flake.nix"
  require_contains "$target/flake.nix" 'description = "Nixfied thin wrapper";'
  require_contains "$target/flake.nix" 'nixfied.url = "github:willyrgf/nixfied/dev";'
  require_contains "$target/flake.nix" 'projectModules = [ ./nixfied/project/module.nix ];'

  if [ -d "$target/nixfied" ]; then
    fail "thin wrapper should not vendor framework sources"
  fi

  if ${pkgs.gnugrep}/bin/grep -Fq 'nixfiedLib = import ./nixfied/lib/default.nix {' "$target/flake.nix"; then
    fail "thin wrapper should not import vendored lib/default.nix"
  fi

  set +e
  "$ORCH" run-task task.framework.install --target > "$TMPDIR/missing-target.out" 2>&1
  missing_target_rc="$?"
  set -e
  if [ "$missing_target_rc" -eq 0 ]; then
    fail "expected --target without value to fail"
  fi
  require_contains "$TMPDIR/missing-target.out" "option '--target' requires a value"

  set +e
  "$ORCH" run-task task.framework.install --no-such-flag > "$TMPDIR/unknown-arg.out" 2>&1
  unknown_arg_rc="$?"
  set -e
  if [ "$unknown_arg_rc" -eq 0 ]; then
    fail "expected unknown install flag to fail"
  fi
  require_contains "$TMPDIR/unknown-arg.out" "unknown option '--no-such-flag' for task 'task.framework.install'"

  echo "OK: framework::install thin wrapper and arg validation are covered" > "$out"
''
