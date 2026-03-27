{
  pkgs,
  model,
  services,
  registry,
}:
let
  harness = import ./lib/harness.nix {
    inherit
      pkgs
      model
      services
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
  sourceRoot = builtins.toString ../..;
in
pkgs.runCommand "framework-template-install-upgrade-help-smoke" { } ''
  set -euo pipefail
  ${harness.shellPrelude}

  EXECUTOR="${executor}/bin/nixfied-executor"
  NIX_BIN="${pkgs.nix}/bin/nix"
  GIT_BIN="${pkgs.git}/bin/git"

  export REGISTRY_ROOT="$TMPDIR/registry"
  mkdir -p "$REGISTRY_ROOT"
  export NIXFIED_RUNTIME_DIR_SCOPE_OVERRIDE="$TMPDIR/runtime-scope"
  mkdir -p "$NIXFIED_RUNTIME_DIR_SCOPE_OVERRIDE"
  export HOME="$TMPDIR/home"
  export XDG_CACHE_HOME="$HOME/.cache"
  mkdir -p "$XDG_CACHE_HOME"

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

  run_nix_checked() {
    local out_file="$1"
    local cwd="$2"
    local subcmd="$3"
    shift 3
    set +e
    (
      cd "$cwd"
      "$NIX_BIN" \
        --extra-experimental-features nix-command \
        --extra-experimental-features flakes \
        "$subcmd" \
        --offline \
        --no-write-lock-file \
        "$@"
    ) > "$out_file" 2>&1
    local rc="$?"
    set -e
    if [ "$rc" -ne 0 ]; then
      echo "nix command failed (rc=$rc) in $cwd: nix $*"
      cat "$out_file"
      fail "nix command invocation failed"
    fi
  }

  template_repo="$TMPDIR/template-repo"
  mkdir -p "$template_repo"
  "$GIT_BIN" -C "$template_repo" init -q
  echo "# template repo" > "$template_repo/README.md"

  run_task_checked "$TMPDIR/install-bootstrap.out" task.framework.install --vendor --target "$template_repo"

  require_file "$template_repo/flake.nix"
  require_file "$template_repo/nixfied/project/module.nix"
  require_file "$template_repo/nixfied/framework/core/default.nix"
  require_file "$template_repo/nixfied/framework/runtime/default.nix"
  require_file "$template_repo/nixfied/VENDORED.txt"
  require_contains "$template_repo/nixfied/VENDORED.txt" "Framework source revision (install/upgrade):"
  require_contains "$template_repo/nixfied/VENDORED.txt" "Recent framework changes:"
  require_not_contains "$template_repo/nixfied/VENDORED.txt" 'set by `framework::install` / `framework::upgrade`'
  require_not_contains "$template_repo/nixfied/VENDORED.txt" "- unknown"
  if ! ${pkgs.gnugrep}/bin/grep -Eq '^- [0-9a-f]{7,}(-dirty)?$' "$template_repo/nixfied/VENDORED.txt"; then
    fail "vendored install must record the framework source revision"
  fi

  if [ -f "$template_repo/.workspace" ]; then
    fail "vendored install must not include repo-root .workspace"
  fi

  cp -f "${sourceRoot}/flake.lock" "$template_repo/flake.lock"
  run_task_checked "$TMPDIR/install-rerun.out" task.framework.install --vendor --target "$template_repo"
  run_task_checked "$TMPDIR/upgrade-command.out" task.framework.upgrade --target "$template_repo"

  require_contains "$TMPDIR/install-rerun.out" "OK: vendored wrapper upgraded at $template_repo/flake.nix"
  require_contains "$TMPDIR/upgrade-command.out" "OK: vendored wrapper upgraded at $template_repo/flake.nix"
  if [ -f "$template_repo/.workspace" ]; then
    fail "upgrade must keep repo-root .workspace absent"
  fi
  if ! ${pkgs.gnugrep}/bin/grep -Eq '^- [0-9a-f]{7,}(-dirty)?$' "$template_repo/nixfied/VENDORED.txt"; then
    fail "vendored upgrade must keep the framework source revision"
  fi
  require_contains "$template_repo/nixfied/VENDORED.txt" "Changes since previous vendored revision"
  require_contains "$template_repo/nixfied/VENDORED.txt" "previous vendored revision already matches current framework revision"

  "$GIT_BIN" -C "$template_repo" add -A
  "$GIT_BIN" -C "$template_repo" -c user.name=nixfied -c user.email=nixfied@example.invalid \
    commit -qm "snapshot vendored wrapper"
  run_nix_checked "$TMPDIR/help.out" "$template_repo" run .#help
  run_nix_checked "$TMPDIR/upgrade-help.out" "$template_repo" run .#framework::upgrade -- --help
  run_nix_checked "$TMPDIR/upgrade-app-first.out" "${sourceRoot}" run .#framework::upgrade -- --target "$template_repo"
  run_nix_checked "$TMPDIR/upgrade-app-second.out" "${sourceRoot}" run .#framework::upgrade -- --target "$template_repo"
  run_nix_checked "$TMPDIR/framework-test-help.out" "${sourceRoot}" run .#framework::test -- --help
  require_contains "$TMPDIR/help.out" "Core apps:"
  require_contains "$TMPDIR/help.out" "framework::upgrade - Upgrade vendored wrapper in-place"
  require_not_contains "$TMPDIR/help.out" "framework::install -"
  require_not_contains "$TMPDIR/help.out" "framework::test -"
  require_contains "$TMPDIR/upgrade-help.out" "framework::upgrade - Upgrade vendored wrapper in-place"
  require_contains "$TMPDIR/upgrade-help.out" "Usage:"
  require_contains "$TMPDIR/upgrade-help.out" "  nix run .#framework::upgrade -- --target ."
  require_contains "$TMPDIR/upgrade-help.out" "  --target <string>: Output directory for generated wrapper."
  require_contains "$TMPDIR/upgrade-help.out" "  --reset-project: When vendoring, overwrite nixfied/project."
  require_contains "$TMPDIR/upgrade-help.out" "Do not target a framework workspace root itself"
  require_contains "$TMPDIR/upgrade-help.out" "  -h, --help: Show this help."
  require_contains "$TMPDIR/upgrade-app-first.out" "INFO: upgrading vendored wrapper (preserving nixfied/project/ and nixfied/local/)"
  require_contains "$TMPDIR/upgrade-app-first.out" "OK: vendored wrapper upgraded at $template_repo/flake.nix"
  require_contains "$TMPDIR/upgrade-app-second.out" "INFO: upgrading vendored wrapper (preserving nixfied/project/ and nixfied/local/)"
  require_contains "$TMPDIR/upgrade-app-second.out" "OK: vendored wrapper upgraded at $template_repo/flake.nix"
  require_contains "$TMPDIR/framework-test-help.out" "framework::test - Run framework validation in the model"
  require_contains "$TMPDIR/framework-test-help.out" "Usage:"
  require_contains "$TMPDIR/framework-test-help.out" "  nix run .#framework::test"
  require_contains "$TMPDIR/framework-test-help.out" "migration"

  echo "OK: template install/upgrade/help downstream contract is validated" > "$out"
''
