# Framework test runner app
{ pkgs, lib }:

let
  extraPath = pkgs.lib.makeBinPath [
    pkgs.bash
    pkgs.coreutils
    pkgs.findutils
    pkgs.gawk
    pkgs.gnugrep
    pkgs.gnused
    pkgs.git
    pkgs.nix
    pkgs.rsync
  ];
  testScript = pkgs.writeShellScript "framework-test" ''
    set -euo pipefail

    ROOT=$(git rev-parse --show-toplevel 2>/dev/null || pwd)
    ROOT=$(cd "$ROOT" && pwd -P)
    cd "$ROOT"

    if [ ! -f "$ROOT/flake.nix" ] || [ ! -d "$ROOT/nix" ]; then
      echo "Run from the framework repository root." >&2
      exit 1
    fi

    if ! command -v nix >/dev/null 2>&1; then
      echo "nix is required in PATH" >&2
      exit 1
    fi

    if ! command -v git >/dev/null 2>&1; then
      echo "git is required in PATH" >&2
      exit 1
    fi

    WORKDIR=$(mktemp -d)
    WORKDIR=$(cd "$WORKDIR" && pwd -P)
    SYSTEM="${pkgs.stdenv.hostPlatform.system}"

    cleanup() {
      if [ "''${NIXFIED_TEST_KEEP:-}" = "1" ]; then
        echo "Keeping test workspace: $WORKDIR"
      else
        rm -rf "$WORKDIR"
      fi
    }
    trap cleanup EXIT

    export XDG_DATA_HOME="$WORKDIR/xdg"

    log() {
      printf '%s\n' "==> $*"
    }

    fail() {
      printf '%s\n' "FAIL: $*" >&2
      exit 1
    }

    assert_file_exists() {
      local path="$1"
      [ -f "$path" ] || fail "expected file: $path"
    }

    assert_file_absent() {
      local path="$1"
      [ ! -e "$path" ] || fail "unexpected file: $path"
    }

    assert_contains() {
      local file="$1"
      local pattern="$2"
      grep -q "$pattern" "$file" || fail "expected '$pattern' in $file"
    }

    build_expr() {
      local expr="$1"
      nix build --impure --expr "$expr" --argstr root "$ROOT" --argstr system "$SYSTEM" --no-link --print-out-paths
    }

    init_repo() {
      local dir="$1"
      mkdir -p "$dir"
      (cd "$dir" && git init -q)
    }

    log "flake eval"
    nix flake show "path:$ROOT" >/dev/null
    nix flake check --no-build "path:$ROOT" >/dev/null

    log "core apps"
    HELP_OUT="$WORKDIR/help.txt"
    nix run "path:$ROOT"#help > "$HELP_OUT"
    assert_contains "$HELP_OUT" "Commands:"
    assert_contains "$HELP_OUT" "PROJECT_ENV"
    assert_contains "$HELP_OUT" "NIX_ENV"

    nix run "path:$ROOT"#dev >/dev/null
    nix run "path:$ROOT"#test >/dev/null
    nix run "path:$ROOT"#build >/dev/null
    nix run "path:$ROOT"#check >/dev/null
    nix run "path:$ROOT"#ci -- --summary >/dev/null

    log "ci DSL fixture"
    CI_DIR="$WORKDIR/ci"
    mkdir -p "$CI_DIR"
    CI_EXPR=$(cat <<'NIX'
    { root, system }:
    let
      flake = builtins.getFlake root;
      pkgs = flake.inputs.nixpkgs.legacyPackages.''${system};
      base = import ./nix/project { inherit pkgs; };
      fixture = import ./tests/framework/fixtures/ci/ci.nix { project = base.project; };
      project = pkgs.lib.recursiveUpdate base fixture;
      slots = import ./nix/slots.nix { inherit pkgs project; };
      hooks = import ./nix/hooks.nix { inherit pkgs project slots; postgres = null; nginx = null; };
      lib = import ./nix/lib.nix { inherit pkgs project hooks; };
      ciEntry = import ./nix/ci.nix { inherit pkgs project lib; };
    in
      ciEntry.scriptDrv
    NIX
    )

    CI_SCRIPT=$(build_expr "$CI_EXPR")
    unset CI_MISSING
    (cd "$CI_DIR" && "$CI_SCRIPT" --mode basic >/dev/null)
    assert_file_exists "$CI_DIR/.ci-artifacts/runs.ok"
    assert_file_absent "$CI_DIR/.ci-artifacts/skip-missing.ok"
    assert_file_absent "$CI_DIR/.ci-artifacts/when.ok"
    assert_file_absent "$CI_DIR/.ci-artifacts/requires.ok"

    set +e
    (cd "$CI_DIR" && "$CI_SCRIPT" --mode failure >/dev/null 2>&1)
    RC=$?
    set -e
    if [ "$RC" -eq 0 ]; then
      fail "expected CI failure mode to exit non-zero"
    fi
    assert_file_exists "$CI_DIR/.ci-artifacts/fail.cleanup"

    log "module hooks fixture"
    MOD_DIR="$WORKDIR/modules"
    mkdir -p "$MOD_DIR"
    MOD_EXPR=$(cat <<'NIX'
    { root, system }:
    let
      flake = builtins.getFlake root;
      pkgs = flake.inputs.nixpkgs.legacyPackages.''${system};
      base = import ./nix/project { inherit pkgs; };
      conf = import ./tests/framework/fixtures/modules/conf.nix { inherit pkgs; };
      dev = import ./tests/framework/fixtures/modules/dev.nix { project = conf.project; };
      project = pkgs.lib.recursiveUpdate base (pkgs.lib.recursiveUpdate conf dev);
      slots = import ./nix/slots.nix { inherit pkgs project; };
      postgres =
        if (project.modules.postgres.enable or false) then
          import ./nix/postgres.nix { inherit pkgs project slots; }
        else
          null;
      nginx =
        if (project.modules.nginx.enable or false) then
          import ./nix/nginx.nix { inherit pkgs project slots; }
        else
          null;
      hooks = import ./nix/hooks.nix { inherit pkgs project slots postgres nginx; };
      lib = import ./nix/lib.nix { inherit pkgs project hooks; };
    in
      let
        devCfg = dev.commands.dev or { };
      in
      lib.mkAppScript {
        name = "dev";
        script = devCfg.script or "";
        env = devCfg.env or { };
        useDeps = devCfg.useDeps or false;
      }
    NIX
    )

    DEV_SCRIPT=$(build_expr "$MOD_EXPR")
    DEV_LOG="$WORKDIR/dev-fixture.log"
    set +e
    (
      unset SLOT_INFO REQUIRE_SLOT_ENV \
        POSTGRES_INIT POSTGRES_START POSTGRES_STOP POSTGRES_SETUP_DB POSTGRES_FULL_START POSTGRES_FULL_START_TEST \
        NGINX_INIT NGINX_START NGINX_STOP NGINX_SITE_PROXY NGINX_SITE_STATIC
      export NIXFIED_TEST_DEBUG=1
      cd "$MOD_DIR" && "$DEV_SCRIPT" >"$DEV_LOG" 2>&1
    )
    DEV_RC=$?
    set -e
    if [ "$DEV_RC" -ne 0 ]; then
      echo "Module fixture failed (rc=$DEV_RC)." >&2
      echo "" >&2
      echo "Script excerpt:" >&2
      nl -ba "$DEV_SCRIPT" | sed -n '60,100p' >&2
      echo "" >&2
      echo "Fixture output (last 50 lines):" >&2
      tail -50 "$DEV_LOG" >&2 || true
      echo "" >&2
      echo "Matches for 'name}' in script:" >&2
      grep -n "name}" "$DEV_SCRIPT" >&2 || true
      exit "$DEV_RC"
    fi

    log "supervisor config"
    SUP_EXPR=$(cat <<'NIX'
    { root, system }:
    let
      flake = builtins.getFlake root;
      pkgs = flake.inputs.nixpkgs.legacyPackages.''${system};
      base = import ./nix/project { inherit pkgs; };
      project = pkgs.lib.recursiveUpdate base {
        supervisor = {
          enable = true;
          services = {
            app = {
              command = "echo hello";
              workingDir = ".";
            };
          };
        };
      };
      slots = import ./nix/slots.nix { inherit pkgs project; };
      supervisor = import ./nix/supervisor.nix { inherit pkgs project slots; };
    in
      supervisor.generateConfig
    NIX
    )

    SUP_SCRIPT=$(build_expr "$SUP_EXPR")
    SUP_CONFIG=$("$SUP_SCRIPT")
    assert_file_exists "$SUP_CONFIG"
    assert_contains "$SUP_CONFIG" "processes:"
    assert_contains "$SUP_CONFIG" "app:"

    export NIXFIED_PROMPT_PLAN=0
    log "installer basic"
    INSTALL_BASE="$WORKDIR/install-repo"
    init_repo "$INSTALL_BASE"
    (cd "$INSTALL_BASE" && nix run "path:$ROOT"#framework::install >/dev/null)
    INSTALL_TARGET="''${INSTALL_BASE}_nixified"
    assert_file_exists "$INSTALL_TARGET/flake.nix"
    if [ ! -d "$INSTALL_TARGET/nix" ]; then
      fail "expected nix/ directory in installer target"
    fi

    log "installer filter"
    INSTALL_FILTER="$WORKDIR/install-filter"
    init_repo "$INSTALL_FILTER"
    (cd "$INSTALL_FILTER" && nix run "path:$ROOT"#framework::install -- --filter=conf,ci >/dev/null)
    FILTER_TARGET="''${INSTALL_FILTER}_nixified"
    assert_file_exists "$FILTER_TARGET/nix/project/ci.nix"
    assert_file_absent "$FILTER_TARGET/nix/project/dev.nix"
    assert_file_absent "$FILTER_TARGET/nix/project/test.nix"
    assert_file_absent "$FILTER_TARGET/nix/project/prod.nix"
    assert_file_absent "$FILTER_TARGET/nix/project/quality.nix"
    if grep -q "dev.nix" "$FILTER_TARGET/nix/project/default.nix"; then
      fail "default.nix should not include dev.nix when filtered"
    fi

    log "installer force"
    INSTALL_FORCE="$WORKDIR/force_nixified"
    init_repo "$INSTALL_FORCE"
    mkdir -p "$INSTALL_FORCE/nix"
    (cd "$INSTALL_FORCE" && nix run "path:$ROOT"#framework::install -- --force >/dev/null)
    assert_file_exists "$INSTALL_FORCE/flake.nix"
    if [ ! -d "$INSTALL_FORCE/nix" ]; then
      fail "expected nix/ directory in force target"
    fi

    log "all tests passed"
  '';

in
{
  test = lib.mkApp {
    name = "test";
    description = "Run framework integration tests";
    env = { };
    useDeps = false;
    script = ''
      export PATH="${extraPath}:$PATH"
      ${pkgs.bash}/bin/bash ${testScript} "$@"
    '';
  };
}
