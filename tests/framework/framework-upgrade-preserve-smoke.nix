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
  executor = import ../../nixfied/framework/runtime/executor.nix {
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

  EXECUTOR="${executor}/bin/nixfied-executor"
  export REGISTRY_ROOT="$TMPDIR/registry"
  export NIXFIED_RUNTIME_DIR_SCOPE_OVERRIDE="$TMPDIR/runtime-scope"
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

  target="$TMPDIR/vendor-wrapper"
  run_task_checked "$TMPDIR/install-initial.out" task.framework.install --vendor --target "$target"

  require_file "$target/nixfied/project/module.nix"
  require_file "$target/nixfied/local/default.nix"
  require_file "$target/nixfied/lib/default.nix"
  require_file "$target/nixfied/VENDORED.txt"
  require_contains "$target/nixfied/VENDORED.txt" "Framework source revision (install/upgrade):"
  require_not_contains "$target/nixfied/VENDORED.txt" 'set by `framework::install` / `framework::upgrade`'
  require_not_contains "$target/nixfied/VENDORED.txt" "- unknown"
  if ! ${pkgs.gnugrep}/bin/grep -Eq '^- [0-9a-f]{7,}(-dirty)?$' "$target/nixfied/VENDORED.txt"; then
    fail "vendored metadata must record a framework source revision"
  fi

  echo "# USER_PROJECT_MARKER" >> "$target/nixfied/project/module.nix"
  echo "# USER_LOCAL_MARKER" >> "$target/nixfied/local/default.nix"
  echo "# USER_LIB_MARKER" >> "$target/nixfied/lib/default.nix"

  run_task_checked "$TMPDIR/install-rerun.out" task.framework.install --vendor --target "$target"

  require_contains "$target/nixfied/project/module.nix" "USER_PROJECT_MARKER"
  require_contains "$target/nixfied/local/default.nix" "USER_LOCAL_MARKER"
  if ${pkgs.gnugrep}/bin/grep -Fq "USER_LIB_MARKER" "$target/nixfied/lib/default.nix"; then
    fail "framework-owned file should be overwritten during vendored upgrade"
  fi

  run_task_checked "$TMPDIR/upgrade-default.out" task.framework.upgrade --target "$target"
  require_contains "$target/nixfied/project/module.nix" "USER_PROJECT_MARKER"
  require_contains "$target/nixfied/local/default.nix" "USER_LOCAL_MARKER"
  if ! ${pkgs.gnugrep}/bin/grep -Eq '^- [0-9a-f]{7,}(-dirty)?$' "$target/nixfied/VENDORED.txt"; then
    fail "vendored metadata must keep the framework source revision on upgrade"
  fi

  run_task_checked "$TMPDIR/upgrade-reset.out" task.framework.upgrade --target "$target" --reset-project --reset-local
  if ${pkgs.gnugrep}/bin/grep -Fq "USER_PROJECT_MARKER" "$target/nixfied/project/module.nix"; then
    fail "project marker should be removed by --reset-project"
  fi
  if ${pkgs.gnugrep}/bin/grep -Fq "USER_LOCAL_MARKER" "$target/nixfied/local/default.nix"; then
    fail "local marker should be removed by --reset-local"
  fi

  echo "OK: framework::upgrade preserves user-owned paths by default" > "$out"
''
