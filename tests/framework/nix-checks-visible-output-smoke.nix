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

    case "$1" in
      flake)
        case "''${2:-}" in
          show)
            echo "flake show output"
            exit 0
            ;;
          check)
            echo "flake check output"
            exit 0
            ;;
          *)
            echo "unexpected flake subcommand: $*" >&2
            exit 1
            ;;
        esac
        ;;
      run)
        echo "help output"
        exit 0
        ;;
      *)
        echo "unexpected nix invocation: $*" >&2
        exit 1
        ;;
    esac
  '';

  fakePkgs = pkgs // {
    deadnix = fakeDeadnix;
    nix = fakeNix;
    nil = fakeNil;
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
pkgs.runCommand "nix-checks-visible-output-smoke" { } ''
  set -euo pipefail

  repo="$TMPDIR/repo"
  mkdir -p "$repo"
  cat > "$repo/flake.nix" <<'EOF'
  {
    description = "fixture";
  }
  EOF

  (
    cd "$repo"
    unset NIX_BUILD_TOP
    unset NIXFIED_PARENT_WORKFLOW_ID
    ${fakeNixChecksPkg}/bin/nix-checks --mode full > "$TMPDIR/nix-checks.out" 2>&1
  )

  ${pkgs.gnugrep}/bin/grep -Fq "flake show output" "$TMPDIR/nix-checks.out"
  ${pkgs.gnugrep}/bin/grep -Fq "help output" "$TMPDIR/nix-checks.out"
  ${pkgs.gnugrep}/bin/grep -Fq "flake check output" "$TMPDIR/nix-checks.out"

  echo "OK: nix-checks emits flake show, help, and flake check output" > "$out"
''
