# Vendored Helios package wrapper
{ pkgs }:

pkgs.writeShellScriptBin "helios" ''
  set -euo pipefail

  if [ -n "''${HELIOS_BIN:-}" ]; then
    if [ ! -x "''${HELIOS_BIN}" ]; then
      echo "ERROR: HELIOS_BIN is set but not executable path=$HELIOS_BIN" >&2
      exit 1
    fi
    exec "''${HELIOS_BIN}" "$@"
  fi

  SYS_HELIOS=$(command -v helios 2>/dev/null || true)
  if [ -n "$SYS_HELIOS" ] && [ "$SYS_HELIOS" != "$0" ]; then
    exec "$SYS_HELIOS" "$@"
  fi

  echo "ERROR: Helios binary unavailable. Override modules.helios.package or set HELIOS_BIN." >&2
  echo "ERROR: Suggested source: https://github.com/a16z/helios" >&2
  exit 1
''
