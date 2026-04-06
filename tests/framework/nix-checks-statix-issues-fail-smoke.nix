{ pkgs }:
let
  fakeFormatter = pkgs.writeShellScriptBin "nixfmt" ''
    set -euo pipefail
    if [ "$#" -lt 1 ] || [ "$1" != "--check" ]; then
      echo "unexpected nixfmt args: $*" >&2
      exit 1
    fi
  '';

  fakeDeadnix = pkgs.writeShellScriptBin "deadnix" ''
    set -euo pipefail
    if [ "$#" -lt 1 ]; then
      echo "unexpected deadnix args: $*" >&2
      exit 1
    fi
  '';

  fakeStatix = pkgs.writeShellScriptBin "statix" ''
    set -euo pipefail

    if [ "$#" -lt 2 ] || [ "$1" != "check" ]; then
      echo "unexpected statix args: $*" >&2
      exit 1
    fi

    shift
    for path in "$@"; do
      if [ "$path" = "." ] || [ "$path" = "./flake.nix" ]; then
        echo "warning: redundant inherit"
        exit 1
      fi
    done
  '';

  fakeNil = pkgs.writeShellScriptBin "nil" ''
    set -euo pipefail

    if [ "$#" -lt 2 ] || [ "$1" != "diagnostics" ]; then
      echo "unexpected nil args: $*" >&2
      exit 1
    fi
  '';

  fakeNix = pkgs.writeShellScriptBin "nix" ''
    set -euo pipefail
    echo "unexpected nix invocation: $*" >&2
    exit 99
  '';

  fakePkgs = pkgs // {
    deadnix = fakeDeadnix;
    nil = fakeNil;
    nix = fakeNix;
    statix = fakeStatix;
  };

  fakeNixChecksPkg =
    import ../../nixfied/framework/core/mkNixChecks.nix
      {
        pkgs = fakePkgs;
        lib = pkgs.lib;
      }
      {
        formatterPkg = fakeFormatter;
      };
in
pkgs.runCommand "nix-checks-statix-issues-fail-smoke" { } ''
  set -euo pipefail

  repo="$TMPDIR/repo"
  mkdir -p "$repo"
  cat > "$repo/flake.nix" <<'EOF'
  {
    description = "fixture";
  }
  EOF
  cat > "$repo/extra.nix" <<'EOF'
  { }:
  { }
  EOF

  set +e
  (
    cd "$repo"
    ${fakeNixChecksPkg}/bin/nix-checks --mode full > "$TMPDIR/nix-checks.out" 2>&1
  )
  rc="$?"
  set -e

  if [ "$rc" -eq 0 ]; then
    cat "$TMPDIR/nix-checks.out"
    exit 1
  fi

  ${pkgs.gnugrep}/bin/grep -Fq "ERROR: statix reported issues files=1" "$TMPDIR/nix-checks.out"
  ${pkgs.gnugrep}/bin/grep -Fq "ERROR: statix reported file=./flake.nix" "$TMPDIR/nix-checks.out"
  if ${pkgs.gnugrep}/bin/grep -Fq "unexpected nix invocation" "$TMPDIR/nix-checks.out"; then
    cat "$TMPDIR/nix-checks.out"
    exit 1
  fi

  echo "OK: statix issues fail nix-checks before flake commands" > "$out"
''
