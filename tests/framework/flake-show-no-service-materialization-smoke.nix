{ pkgs }:
let
  lib = pkgs.lib;
  shellHelpers = import ./lib/shell-helpers.nix { inherit pkgs; };
  repoRoot = builtins.toString ../..;
in
pkgs.runCommand "flake-show-no-service-materialization-smoke" { } ''
  set -euo pipefail
  ${shellHelpers.shellPrelude}

  flake_dir="$TMPDIR/flake-show-fixture"
  home_dir="$TMPDIR/home"
  cache_dir="$TMPDIR/cache"
  mkdir -p "$flake_dir"
  mkdir -p "$home_dir" "$cache_dir"

  cat > "$flake_dir/flake.nix" <<'EOF'
  {
    inputs.nixpkgs.url = "path:${pkgs.path}";
    inputs.framework.url = "path:${repoRoot}";

    outputs = { self, nixpkgs, framework }:
      let
        system = ${builtins.toJSON pkgs.system};
        pkgs = import nixpkgs { inherit system; };
        repoRoot = framework.outPath;
        frameworkLib = import "''${framework.outPath}/nixfied/framework/core" {
          inherit pkgs system;
        };
        frameworkOutputs = frameworkLib.mkFlakeOutputs {
          projectRoot = repoRoot;
          projectModules = [ "''${framework.outPath}/nixfied/project/module.nix" ];
          extraModules = [ "''${framework.outPath}/tests/framework/launcher-helios-task-module.nix" ];
          localOverrides = [ "''${framework.outPath}/tests/framework/poison-helios-source-override.nix" ];
        };
      in
      {
        apps."''${system}" = frameworkOutputs.apps;
      };
  }
  EOF

  HOME="$home_dir" XDG_CACHE_HOME="$cache_dir" ${pkgs.nix}/bin/nix flake show --no-write-lock-file --json "$flake_dir" > "$TMPDIR/flake-show.out" 2>&1 || {
    cat "$TMPDIR/flake-show.out"
    fail "flake show should stay on the cheap public surface"
  }

  require_contains "$TMPDIR/flake-show.out" "\"run-task\""
  require_contains "$TMPDIR/flake-show.out" "\"svc::helios::status\""
  require_not_contains "$TMPDIR/flake-show.out" "service package resolved unexpectedly"

  echo "OK: flake show avoids service materialization on public launcher surfaces" > "$out"
''
