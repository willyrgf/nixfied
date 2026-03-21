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

  sourceRoot = builtins.toString ../..;
in
pkgs.runCommand "runtime-controls-no-service-materialization-smoke" { } ''
  set -euo pipefail
  ${shellHelpers.shellPrelude}

  INSTALL_APP="${frameworkOutputs.apps."framework::install".program}"
  NIX_BIN="${pkgs.nix}/bin/nix"
  GIT_BIN="${pkgs.git}/bin/git"

  export REGISTRY_ROOT="$TMPDIR/registry"
  export HOME="$TMPDIR/home"
  export XDG_CACHE_HOME="$HOME/.cache"
  mkdir -p "$REGISTRY_ROOT" "$XDG_CACHE_HOME"

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
      fail "runtime control invocation failed"
    fi
  }

  target_repo="$TMPDIR/runtime-controls-target"
  mkdir -p "$target_repo/nixfied/project"
  "$GIT_BIN" -C "$target_repo" init -q
  echo '{ description = "runtime control target"; }' > "$target_repo/flake.nix"

  (
    cd "$target_repo"
    "$INSTALL_APP" --vendor > "$TMPDIR/install.out" 2>&1
  ) || {
    cat "$TMPDIR/install.out"
    fail "framework::install bootstrap failed"
  }

  cp -f "${sourceRoot}/flake.lock" "$target_repo/flake.lock"

  cat > "$target_repo/nixfied/project/poison-package.nix" <<'EOF'
{ }:
throw "service package resolved unexpectedly during runtime controls"
EOF

  cat > "$target_repo/nixfied/project/module.nix" <<'EOF'
{ ... }:
{
  nixfied.services.helios = {
    enable = true;
    sourceKeys = [ "poison" ];
    defaultSource = "poison";
    sources.poison.packageFactory = ./poison-package.nix;
  };
}
EOF

  "$GIT_BIN" -C "$target_repo" add flake.nix flake.lock nixfied

  run_nix_checked "$TMPDIR/runs.out" "$target_repo" run .#runs
  require_contains "$TMPDIR/runs.out" "INFO: no runs"

  run_nix_checked "$TMPDIR/stop-all.out" "$target_repo" run .#stop-all-runs
  require_not_contains "$TMPDIR/runs.out" "service package resolved unexpectedly during runtime controls"
  require_not_contains "$TMPDIR/stop-all.out" "service package resolved unexpectedly during runtime controls"

  echo "OK: runtime controls stay off service materialization paths" > "$out"
''
