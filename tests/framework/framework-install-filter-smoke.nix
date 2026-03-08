{ pkgs }:
let
  loggingPrelude = ''
    log_info() { printf 'INFO: %s\n' "$*"; }
    log_warn() { printf 'WARN: %s\n' "$*"; }
    log_error() { printf 'ERROR: %s\n' "$*"; }
    log_ok() { printf 'OK: %s\n' "$*"; }
    log_skip() { printf 'SKIP: %s\n' "$*"; }
  '';

  libStub = {
    inherit loggingPrelude;
    appApi.mkNixfiedApp =
      {
        name,
        script,
        env ? { },
        ...
      }:
      let
        envExports = pkgs.lib.concatStringsSep "\n" (
          pkgs.lib.mapAttrsToList (envName: envValue: "export ${envName}=${pkgs.lib.escapeShellArg (toString envValue)}") env
        );
        drv = pkgs.writeShellScriptBin name ''
          set -euo pipefail
          ${envExports}
          ${script}
        '';
      in
      {
        type = "app";
        program = "${drv}/bin/${name}";
      };
  };

  frameworkFixture = pkgs.runCommand "framework-install-fixture" { } ''
    mkdir -p "$out/nixfied/project" "$out/nixfied/.framework"
    cat > "$out/flake.nix" <<'EOF'
    {
      description = "fixture";
    }
    EOF
    echo "# fixture framework" > "$out/README.md"
    cat > "$out/nixfied/project/conf.nix" <<'EOF'
    { pkgs ? null }:
    {
      project.name = "fixture";
    }
    EOF
    for template in dev test prod quality ci; do
      cat > "$out/nixfied/project/$template.nix" <<EOF
    { pkgs ? null, project ? { }, commandLib ? null }:
    {
      commands.$template = true;
    }
    EOF
    done
    cat > "$out/nixfied/project/default.nix" <<'EOF'
    { pkgs ? null }: { }
    EOF
    echo "framework placeholder" > "$out/nixfied/.framework/README.md"
  '';

  installApps = import ../../nixfied/.framework/internal/install.nix {
    inherit pkgs;
    lib = libStub;
    frameworkRoot = frameworkFixture;
    frameworkRevision = "test-rev";
  };
in
pkgs.runCommand "framework-install-filter-smoke" { } ''
  set -euo pipefail

  INSTALL_BIN="${installApps.install.program}"
  GIT_BIN="${pkgs.git}/bin/git"

  export HOME="$TMPDIR/home"
  export XDG_CACHE_HOME="$TMPDIR/cache"
  mkdir -p "$HOME" "$XDG_CACHE_HOME"

  fail() {
    echo "ERROR: $*"
    exit 1
  }

  require_file() {
    local path="$1"
    [ -f "$path" ] || fail "missing file: $path"
  }

  require_not_file() {
    local path="$1"
    [ ! -f "$path" ] || fail "unexpected file: $path"
  }

  require_contains() {
    local path="$1"
    local needle="$2"
    ${pkgs.gnugrep}/bin/grep -Fq -- "$needle" "$path" || {
      echo "--- $path"
      cat "$path"
      fail "expected '$needle' in $path"
    }
  }

  require_not_contains() {
    local path="$1"
    local needle="$2"
    if ${pkgs.gnugrep}/bin/grep -Fq -- "$needle" "$path"; then
      echo "--- $path"
      cat "$path"
      fail "unexpected '$needle' in $path"
    fi
  }

  init_repo() {
    local path="$1"
    mkdir -p "$path"
    "$GIT_BIN" -C "$path" init -q
    "$GIT_BIN" -C "$path" config user.email test@example.com
    "$GIT_BIN" -C "$path" config user.name "Nixfied Test"
    echo "# test repo" > "$path/README.md"
    "$GIT_BIN" -C "$path" add README.md
    "$GIT_BIN" -C "$path" commit -qm "init"
  }

  filtered_repo="$TMPDIR/filtered-repo"
  init_repo "$filtered_repo"

  (
    cd "$filtered_repo"
    "$INSTALL_BIN" --filter=build,quality --no-prompt-plan > "$TMPDIR/install-filter.out" 2>&1
  ) || {
    cat "$TMPDIR/install-filter.out"
    fail "filtered install failed"
  }

  require_file "$filtered_repo/nixfied/project/conf.nix"
  require_file "$filtered_repo/nixfied/project/prod.nix"
  require_file "$filtered_repo/nixfied/project/quality.nix"
  require_not_file "$filtered_repo/nixfied/project/dev.nix"
  require_not_file "$filtered_repo/nixfied/project/test.nix"
  require_not_file "$filtered_repo/nixfied/project/ci.nix"
  require_contains "$filtered_repo/nixfied/project/default.nix" "(mkPart ./prod.nix)"
  require_contains "$filtered_repo/nixfied/project/default.nix" "(mkPart ./quality.nix)"
  require_not_contains "$filtered_repo/nixfied/project/default.nix" "(mkPart ./dev.nix)"
  require_not_contains "$filtered_repo/nixfied/project/default.nix" "(mkPart ./test.nix)"
  require_not_contains "$filtered_repo/nixfied/project/default.nix" "(mkPart ./ci.nix)"

  invalid_repo="$TMPDIR/invalid-filter-repo"
  init_repo "$invalid_repo"

  set +e
  (
    cd "$invalid_repo"
    "$INSTALL_BIN" --filter=bogus --no-prompt-plan > "$TMPDIR/install-invalid-filter.out" 2>&1
  )
  invalid_rc="$?"
  set -e

  if [ "$invalid_rc" -eq 0 ]; then
    cat "$TMPDIR/install-invalid-filter.out"
    fail "invalid filter install should fail"
  fi

  require_contains "$TMPDIR/install-invalid-filter.out" "ERROR: Unknown filter: bogus"

  echo "OK: framework install filter planning is data-driven and stable" > "$out"
''
