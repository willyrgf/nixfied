# Installer app for the framework
{
  pkgs,
  lib,
  frameworkRoot,
}:

let
  inherit (lib) mkApp;

  installScript = ''
    set -euo pipefail

    FORCE=false
    FILTERS_RAW=""

    for arg in "$@"; do
      case "$arg" in
        --force) FORCE=true ;;
        --filter=*)
          FILTERS_RAW="''${arg#--filter=}"
          ;;
        --help|-h)
          echo "Usage: nix run github:willyrgf/nixfied#framework::install [--force] [--filter=conf,dev,test,prod,quality,ci]"
          exit 0
          ;;
      esac
    done

    for arg in "$@"; do
      case "$arg" in
        --filter)
          shift
          FILTERS_RAW="''${1:-}"
          ;;
      esac
    done

    if [ "$FORCE" = "true" ]; then
      export NIXFIED_INSTALL_FORCE=1
    fi

    ROOT=$(git rev-parse --show-toplevel 2>/dev/null || true)
    if [ -z "$ROOT" ]; then
      echo "❌ Not inside a git repository." >&2
      exit 1
    fi

    SUFFIX="_nixified"
    BASE=$(basename "$ROOT")

    if [ -z "''${NIXFIED_INSTALL_REENTRY:-}" ]; then
      if [[ "$BASE" != *"$SUFFIX" ]]; then
        TARGET="''${ROOT}''${SUFFIX}"
        if [ -e "$TARGET" ]; then
          if [ "$FORCE" = "true" ]; then
            echo "⚠️  Target already exists: $TARGET"
            echo "    Reusing existing copy (no new copy made)."
            (cd "$TARGET" && NIXFIED_INSTALL_REENTRY=1 NIXFIED_INSTALL_FORCE=1 "$0" "$@")
            exit 0
          else
            echo "❌ Target already exists: $TARGET" >&2
            echo "   Remove it or rename it, then re-run (or pass --force to reuse)." >&2
            exit 1
          fi
        fi
        echo "📦 Copying repository to $TARGET..."
        if command -v rsync >/dev/null 2>&1; then
          rsync -a "$ROOT/" "$TARGET/"
        else
          cp -a "$ROOT" "$TARGET"
        fi
        echo "✅ Copy complete. Re-running installer in $TARGET"
        (cd "$TARGET" && NIXFIED_INSTALL_REENTRY=1 "$0" "$@")
        exit 0
      fi
    fi

    if [[ "$BASE" != *"$SUFFIX" ]]; then
      echo "❌ For safety, run inside a repository ending with $SUFFIX" >&2
      exit 1
    fi

    SRC="${frameworkRoot}"

    if [ ! -f "$SRC/flake.nix" ] || [ ! -d "$SRC/nix" ]; then
      echo "❌ Framework source is missing required files." >&2
      exit 1
    fi

    NEEDS_OVERWRITE=false
    if [ -e "$ROOT/flake.nix" ] || [ -e "$ROOT/flake.lock" ] || [ -d "$ROOT/nix" ]; then
      NEEDS_OVERWRITE=true
    fi

    if [ "$NEEDS_OVERWRITE" = "true" ] && [ -z "''${NIXFIED_INSTALL_FORCE:-}" ]; then
      if [ -t 0 ]; then
        echo "⚠️  Existing Nix files found in $ROOT"
        echo "    This will overwrite: flake.nix, flake.lock, nix/"
        echo -n "Continue? [y/N]: "
        read -r REPLY
        if [[ ! "$REPLY" =~ ^[Yy]$ ]]; then
          echo "Aborted."
          exit 1
        fi
      else
        echo "❌ Existing Nix files found. Re-run with NIXFIED_INSTALL_FORCE=1 to overwrite." >&2
        exit 1
      fi
    fi

    echo "📦 Installing framework files..."

    cp -f "$SRC/flake.nix" "$ROOT/flake.nix"
    if [ -f "$SRC/flake.lock" ]; then
      cp -f "$SRC/flake.lock" "$ROOT/flake.lock"
    fi

    if [ -d "$ROOT/nix" ]; then
      chmod -R u+w "$ROOT/nix" 2>/dev/null || true
      rm -rf "$ROOT/nix"
    fi

    if command -v rsync >/dev/null 2>&1; then
      rsync -a --chmod=Du+w,Fu+w "$SRC/nix/" "$ROOT/nix/"
    else
      cp -R "$SRC/nix" "$ROOT/nix"
      chmod -R u+w "$ROOT/nix" 2>/dev/null || true
    fi

    chmod -R u+w "$ROOT/nix" 2>/dev/null || true
    if command -v chflags >/dev/null 2>&1; then
      chflags -R nouchg "$ROOT/nix" 2>/dev/null || true
    fi
    if command -v chattr >/dev/null 2>&1; then
      chattr -R -i "$ROOT/nix" 2>/dev/null || true
    fi
    chmod u+w "$ROOT/nix/.framework" 2>/dev/null || true
    if command -v chflags >/dev/null 2>&1; then
      chflags nouchg "$ROOT/nix/.framework" 2>/dev/null || true
    fi
    if command -v chattr >/dev/null 2>&1; then
      chattr -i "$ROOT/nix/.framework" 2>/dev/null || true
    fi
    rm -f "$ROOT/nix/.framework"

    if [ -n "$FILTERS_RAW" ]; then
      IFS=',' read -r -a FILTERS <<< "$FILTERS_RAW"
      declare -A KEEP
      KEEP[conf]=1

      for f in "''${FILTERS[@]}"; do
        f="''${f,,}"
        case "$f" in
          conf|dev|test|prod|quality|ci)
            KEEP["$f"]=1
            ;;
          "")
            ;;
          *)
            echo "❌ Unknown filter: $f" >&2
            exit 1
            ;;
        esac
      done

      for f in dev test prod quality ci; do
        if [ -z "''${KEEP[$f]:-}" ]; then
          rm -f "$ROOT/nix/project/$f.nix" 2>/dev/null || true
        fi
      done

      {
        echo "{ pkgs ? null }:"
        echo ""
        echo "let"
        echo "  conf = import ./conf.nix { inherit pkgs; };"
        echo "  project = conf.project or { };"
        echo "  parts = ["
        echo "    conf"
        for f in dev test prod quality ci; do
          if [ -n "''${KEEP[$f]:-}" ]; then
            echo "    (import ./$f.nix { inherit pkgs project; })"
          fi
        done
        echo "  ];"
        echo "in"
        echo "pkgs.lib.foldl' pkgs.lib.recursiveUpdate { } parts"
      } > "$ROOT/nix/project/default.nix"
    fi

    echo "✅ Framework installed."
    echo "Next:"
    echo "  - Edit nix/project/conf.nix"
    echo "  - Customize nix/project/{dev,test,prod,quality,ci}.nix"
  '';
in
{
  install = mkApp {
    name = "install";
    description = "Install Nixfied framework into a repository";
    env = { };
    useDeps = false;
    script = installScript;
  };
}
