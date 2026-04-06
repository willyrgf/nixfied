{
  pkgs,
  model,
  services,
  serviceDefinitions,
  registry,
}:
let
  inherit (pkgs) lib;
  harness = import ./lib/harness.nix {
    inherit
      pkgs
      model
      services
      serviceDefinitions
      registry
      ;
    projectRoot = ../..;
  };
  executor = import ../../nixfied/framework/runtime/executor.nix {
    inherit
      pkgs
      model
      services
      registry
      ;
    projectRoot = ../..;
  };
  frameworkLib = import ../../nixfied/framework/core {
    inherit pkgs;
    inherit (pkgs) system;
  };
  repoRoot = builtins.toString ../..;
  frameworkOutputs = frameworkLib.mkFlakeOutputs {
    projectRoot = ../..;
    projectModules = [ ../../nixfied/project/module.nix ];
    extraModules = [ ];
    localOverrides = [ ];
  };
in
pkgs.runCommand "framework-upgrade-preserve-smoke" { } ''
  set -euo pipefail
  ${harness.shellPrelude}

  EXECUTOR="${executor}/bin/nixfied-executor"
  UPGRADE_APP="${frameworkOutputs.apps."framework::upgrade".program}"
  export REGISTRY_ROOT="$TMPDIR/registry"
  export NIXFIED_RUNTIME_DIR_SCOPE_OVERRIDE="$TMPDIR/runtime-scope"
  export NIXFIED_FLAKE_ROOT=${lib.escapeShellArg repoRoot}
  mkdir -p "$REGISTRY_ROOT"
  mkdir -p "$NIXFIED_RUNTIME_DIR_SCOPE_OVERRIDE"

  run_task_checked() {
    local out_file="$1"
    shift
    set +e
    "$EXECUTOR" run-task "$@" > "$out_file" 2>&1
    local rc="$?"
    set -e
    if [ "$rc" -ne 0 ]; then
      echo "run-task failed (rc=$rc): $*"
      cat "$out_file"
      fail "run-task invocation failed"
    fi
  }

  run_app_expect_failure() {
    local out_file="$1"
    shift
    set +e
    "$@" > "$out_file" 2>&1
    local rc="$?"
    set -e
    if [ "$rc" -eq 0 ]; then
      echo "expected app command to fail: $*"
      cat "$out_file"
      fail "expected app command failure"
    fi
    printf '%s' "$rc"
  }

  workspace_root="$TMPDIR/framework-workspace-root"
  mkdir -p "$workspace_root"
  : > "$workspace_root/.workspace"
  upgrade_root_rc="$(
    run_app_expect_failure \
      "$TMPDIR/upgrade-live-workspace.out" \
      "$UPGRADE_APP" \
      --target "$workspace_root"
  )"
  if [ "$upgrade_root_rc" -ne 2 ]; then
    echo "--- $TMPDIR/upgrade-live-workspace.out"
    cat "$TMPDIR/upgrade-live-workspace.out"
    fail "framework::upgrade workspace-root refusal must exit with usage code 2 (got $upgrade_root_rc)"
  fi
  require_not_file "$workspace_root/flake.nix"
  require_not_file "$workspace_root/nixfied/VENDORED.txt"

  target="$TMPDIR/vendor-wrapper"
  run_task_checked "$TMPDIR/install-initial.out" task.framework.install --vendor --target "$target"

  require_file "$target/nixfied/project/module.nix"
  require_file "$target/nixfied/local/default.nix"
  require_file "$target/nixfied/framework/core/default.nix"
  require_file "$target/nixfied/framework/runtime/default.nix"
  require_file "$target/nixfied/VENDORED.txt"
  require_contains "$target/nixfied/VENDORED.txt" "Framework source revision (install/upgrade):"
  require_contains "$target/nixfied/VENDORED.txt" "Recent framework changes:"
  require_not_contains "$target/nixfied/VENDORED.txt" 'set by `framework::install` / `framework::upgrade`'
  require_not_contains "$target/nixfied/VENDORED.txt" "- unknown"
  if ! ${pkgs.gnugrep}/bin/grep -Eq '^- [0-9a-f]{7,}(-dirty)?$' "$target/nixfied/VENDORED.txt"; then
    fail "vendored metadata must record a framework source revision"
  fi

  echo "# USER_PROJECT_MARKER" >> "$target/nixfied/project/module.nix"
  echo "# USER_LOCAL_MARKER" >> "$target/nixfied/local/default.nix"
  echo "# USER_RUNTIME_MARKER" >> "$target/nixfied/framework/runtime/default.nix"

  run_task_checked "$TMPDIR/install-rerun.out" task.framework.install --vendor --target "$target"

  require_contains "$target/nixfied/project/module.nix" "USER_PROJECT_MARKER"
  require_contains "$target/nixfied/local/default.nix" "USER_LOCAL_MARKER"
  if ${pkgs.gnugrep}/bin/grep -Fq "USER_RUNTIME_MARKER" "$target/nixfied/framework/runtime/default.nix"; then
    fail "framework-owned file should be overwritten during vendored upgrade"
  fi

  run_task_checked "$TMPDIR/upgrade-default.out" task.framework.upgrade --target "$target"
  require_contains "$target/nixfied/project/module.nix" "USER_PROJECT_MARKER"
  require_contains "$target/nixfied/local/default.nix" "USER_LOCAL_MARKER"
  if ! ${pkgs.gnugrep}/bin/grep -Eq '^- [0-9a-f]{7,}(-dirty)?$' "$target/nixfied/VENDORED.txt"; then
    fail "vendored metadata must keep the framework source revision on upgrade"
  fi
  require_contains "$target/nixfied/VENDORED.txt" "Changes since previous vendored revision"
  require_contains "$target/nixfied/VENDORED.txt" "previous vendored revision already matches current framework revision"

  run_task_checked "$TMPDIR/upgrade-reset.out" task.framework.upgrade --target "$target" --reset-project --reset-local
  if ${pkgs.gnugrep}/bin/grep -Fq "USER_PROJECT_MARKER" "$target/nixfied/project/module.nix"; then
    fail "project marker should be removed by --reset-project"
  fi
  if ${pkgs.gnugrep}/bin/grep -Fq "USER_LOCAL_MARKER" "$target/nixfied/local/default.nix"; then
    fail "local marker should be removed by --reset-local"
  fi

  echo "OK: framework::upgrade preserves user-owned paths by default" > "$out"
''
