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
pkgs.runCommand "framework-upgrade-preserve-smoke" { } ''
  set -euo pipefail
  ${harness.shellPrelude}

  ORCH="${harness.orchestrator}/bin/nixfied-orchestrator"
  export REGISTRY_ROOT="$TMPDIR/registry"
  mkdir -p "$REGISTRY_ROOT"

  target="$TMPDIR/vendor-wrapper"
  "$ORCH" run-task task.framework.install --vendor --target "$target" > "$TMPDIR/install-initial.out" 2>&1

  require_file "$target/nixfied/project/module.nix"
  require_file "$target/nixfied/local/default.nix"
  require_file "$target/nixfied/lib/default.nix"

  echo "# USER_PROJECT_MARKER" >> "$target/nixfied/project/module.nix"
  echo "# USER_LOCAL_MARKER" >> "$target/nixfied/local/default.nix"
  echo "# USER_LIB_MARKER" >> "$target/nixfied/lib/default.nix"

  "$ORCH" run-task task.framework.install --vendor --target "$target" > "$TMPDIR/install-rerun.out" 2>&1

  require_contains "$target/nixfied/project/module.nix" "USER_PROJECT_MARKER"
  require_contains "$target/nixfied/local/default.nix" "USER_LOCAL_MARKER"
  if ${pkgs.gnugrep}/bin/grep -Fq "USER_LIB_MARKER" "$target/nixfied/lib/default.nix"; then
    fail "framework-owned file should be overwritten during vendored upgrade"
  fi
  require_contains "$TMPDIR/install-rerun.out" "OK: vendored wrapper upgraded"

  "$ORCH" run-task task.framework.upgrade --target "$target" > "$TMPDIR/upgrade-default.out" 2>&1
  require_contains "$target/nixfied/project/module.nix" "USER_PROJECT_MARKER"
  require_contains "$target/nixfied/local/default.nix" "USER_LOCAL_MARKER"
  require_contains "$TMPDIR/upgrade-default.out" "OK: vendored wrapper upgraded"

  "$ORCH" run-task task.framework.upgrade --target "$target" --reset-project --reset-local > "$TMPDIR/upgrade-reset.out" 2>&1
  if ${pkgs.gnugrep}/bin/grep -Fq "USER_PROJECT_MARKER" "$target/nixfied/project/module.nix"; then
    fail "project marker should be removed by --reset-project"
  fi
  if ${pkgs.gnugrep}/bin/grep -Fq "USER_LOCAL_MARKER" "$target/nixfied/local/default.nix"; then
    fail "local marker should be removed by --reset-local"
  fi

  echo "OK: framework::upgrade preserves user-owned paths by default" > "$out"
''
