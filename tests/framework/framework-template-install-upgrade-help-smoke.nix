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
  executor = import ../../nixfied/runner/executor.nix {
    inherit
      pkgs
      model
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

  require_not_contains() {
    local file="$1"
    local needle="$2"
    if ${pkgs.gnugrep}/bin/grep -Fq -- "$needle" "$file"; then
      echo "unexpected text '$needle' in $file"
      echo "--- $file"
      cat "$file"
      fail "assertion failed"
    fi
  }

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
  require_file "$template_repo/nixfied/lib/default.nix"
  require_file "$template_repo/nixfied/VENDORED.txt"
  require_contains "$template_repo/nixfied/VENDORED.txt" "Framework source revision (install/upgrade):"
  require_not_contains "$template_repo/nixfied/VENDORED.txt" 'set by `framework::install` / `framework::upgrade`'
  require_not_contains "$template_repo/nixfied/VENDORED.txt" "- unknown"
  if ! ${pkgs.gnugrep}/bin/grep -Eq '^- [0-9a-f]{7,}(-dirty)?$' "$template_repo/nixfied/VENDORED.txt"; then
    fail "vendored install must record the framework source revision"
  fi
  if [ -f "$template_repo/nixfied/.framework/.workspace" ]; then
    fail "vendored install must not include nixfied/.framework/.workspace"
  fi

  cp -f "${sourceRoot}/flake.lock" "$template_repo/flake.lock"
  run_task_checked "$TMPDIR/install-rerun.out" task.framework.install --vendor --target "$template_repo"
  run_task_checked "$TMPDIR/upgrade-command.out" task.framework.upgrade --target "$template_repo"

  require_contains "$TMPDIR/install-rerun.out" "OK: vendored wrapper upgraded at $template_repo/flake.nix"
  require_contains "$TMPDIR/upgrade-command.out" "OK: vendored wrapper upgraded at $template_repo/flake.nix"
  if [ -f "$template_repo/nixfied/.framework/.workspace" ]; then
    fail "upgrade must keep nixfied/.framework/.workspace absent"
  fi
  if ! ${pkgs.gnugrep}/bin/grep -Eq '^- [0-9a-f]{7,}(-dirty)?$' "$template_repo/nixfied/VENDORED.txt"; then
    fail "vendored upgrade must keep the framework source revision"
  fi

  "$GIT_BIN" -C "$template_repo" add -A
  run_nix_checked "$TMPDIR/help.out" "$template_repo" run .#help
  require_contains "$TMPDIR/help.out" "Core apps:"
  require_contains "$TMPDIR/help.out" "framework::upgrade - Upgrade vendored wrapper in-place"
  require_not_contains "$TMPDIR/help.out" "framework::install -"
  require_not_contains "$TMPDIR/help.out" "framework::test -"

  echo "OK: template install/upgrade/help downstream contract is validated" > "$out"
''
