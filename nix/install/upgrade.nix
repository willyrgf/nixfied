# Nixfied upgrade surface, packaged as a shell application.
# The repin script is embedded here (literal `${...}` is escaped as
# `''${...}` for the Nix indented string); `nix run .#upgrade` runs it.
{ pkgs }:
pkgs.writeShellApplication {
  name = "nixfied-upgrade";
  runtimeInputs = [
    pkgs.coreutils
    pkgs.nix
  ];
  text = ''
    set -euo pipefail

    # Nixfied upgrade surface.
    #
    # Ownership boundary:
    #   - Nixfied owns the flake input pin and the import/compile wiring.
    #   - The project owns every semantic declaration in nixfied.nix.
    # This command therefore only ever rewrites the `nixfied.url` input pin and
    # refreshes the lock entry for that input. It never creates, edits, or deletes
    # nixfied.nix, and it makes no compatibility promise for already-compiled models.

    root="."
    nixfied_url=""
    update_lock=1

    usage() {
      echo 'usage: nixfied upgrade [--root PATH] [--nixfied-url URL] [--no-lock]'
    }

    take_value() {
      local flag="$1"
      shift
      if [[ $# -eq 0 || -z "$1" || "$1" == --* ]]; then
        echo "missing $flag value" >&2
        exit 2
      fi
      printf '%s' "$1"
    }

    while [[ $# -gt 0 ]]; do
      case "$1" in
        --root)
          root="$(take_value "$1" "''${2-}")"
          shift 2
          ;;
        --nixfied-url)
          nixfied_url="$(take_value "$1" "''${2-}")"
          shift 2
          ;;
        --no-lock)
          update_lock=0
          shift
          ;;
        -h | --help)
          usage
          exit 0
          ;;
        *)
          echo "unknown upgrade argument: $1" >&2
          usage >&2
          exit 2
          ;;
      esac
    done

    flake="$root/flake.nix"

    if [[ ! -e "$flake" ]]; then
      {
        echo "no Nixfied flake.nix to upgrade in $root"
        echo "No files were changed."
        echo "Run 'nixfied install' first to scaffold a project."
      } >&2
      exit 3
    fi

    flake_eval_path="$(realpath "$flake")"

    has_nixfied_input_pin() {
      local verdict
      if ! verdict="$(
        NIXFIED_UPGRADE_FLAKE="$flake_eval_path" nix eval --impure --expr '
          let
            flake = import (builtins.getEnv "NIXFIED_UPGRADE_FLAKE");
          in
            flake ? inputs && flake.inputs ? nixfied && flake.inputs.nixfied ? url
        ' 2>/dev/null
      )"; then
        return 1
      fi
      [[ "$verdict" == "true" ]]
    }

    if ! has_nixfied_input_pin; then
      {
        echo "flake.nix in $root has no Nixfied input pin to upgrade"
        echo "No files were changed."
        echo "Expected an 'inputs.nixfied.url = \"...\";' assignment or an 'inputs.nixfied = { url = \"...\"; ... };' block."
      } >&2
      exit 3
    fi

    nix_escape() {
      local value="$1"
      value="''${value//\\/\\\\}"
      value="''${value//\"/\\\"}"
      printf '%s' "$value"
    }

    brace_delta() {
      local text="$1"
      local delta=0
      local i
      local ch
      for ((i = 0; i < ''${#text}; i++)); do
        ch="''${text:i:1}"
        case "$ch" in
          "{") delta=$((delta + 1)) ;;
          "}") delta=$((delta - 1)) ;;
        esac
      done
      printf '%s' "$delta"
    }

    is_direct_nixfied_url() {
      local stripped="$1"
      [[ "$stripped" == 'nixfied.url="'*'";'* || "$stripped" == 'inputs.nixfied.url="'*'";'* ]]
    }

    is_nixfied_input_block_start() {
      local stripped="$1"
      [[ "$stripped" == 'nixfied={'* || "$stripped" == 'inputs.nixfied={'* ]]
    }

    changed=""
    preserved=""

    if [[ -e "$root/nixfied.nix" ]]; then
      preserved="nixfied.nix"
    fi

    if [[ -n "$nixfied_url" ]]; then
      nixfied_url_escaped="$(nix_escape "$nixfied_url")"
      rewritten="$root/.nixfied-upgrade.tmp"
      matches=0
      in_nixfied_input_block=0
      nixfied_input_block_depth=0
      : >"$rewritten"
      while IFS= read -r line || [[ -n "$line" ]]; do
        # Compare on a whitespace-stripped form so we match the exact `nixfied.url`
        # assignment regardless of indentation/spacing. Attrset inputs are matched
        # as a scoped block and only their inner `url = "...";` line is rewritten.
        stripped="''${line//[[:space:]]/}"
        if [[ "$in_nixfied_input_block" -eq 0 ]] && is_direct_nixfied_url "$stripped"; then
          if [[ "$stripped" == 'inputs.nixfied.url="'* ]]; then
            indent="''${line%%inputs.nixfied.url*}"
            printf '%sinputs.nixfied.url = "%s";\n' "$indent" "$nixfied_url_escaped" >>"$rewritten"
          else
            indent="''${line%%nixfied.url*}"
            printf '%snixfied.url = "%s";\n' "$indent" "$nixfied_url_escaped" >>"$rewritten"
          fi
          matches=$((matches + 1))
        elif [[ "$in_nixfied_input_block" -eq 1 && "$stripped" == 'url="'*'";'* ]]; then
          indent="''${line%%url*}"
          printf '%surl = "%s";\n' "$indent" "$nixfied_url_escaped" >>"$rewritten"
          matches=$((matches + 1))
        else
          printf '%s\n' "$line" >>"$rewritten"
        fi

        if [[ "$in_nixfied_input_block" -eq 0 ]] && is_nixfied_input_block_start "$stripped"; then
          in_nixfied_input_block=1
          nixfied_input_block_depth="$(brace_delta "$line")"
          if [[ "$nixfied_input_block_depth" -le 0 ]]; then
            in_nixfied_input_block=0
          fi
        elif [[ "$in_nixfied_input_block" -eq 1 ]]; then
          nixfied_input_block_depth=$((nixfied_input_block_depth + $(brace_delta "$line")))
          if [[ "$nixfied_input_block_depth" -le 0 ]]; then
            in_nixfied_input_block=0
          fi
        fi
      done <"$flake"

      if [[ "$matches" -ne 1 ]]; then
        rm -f "$rewritten"
        {
          echo "expected exactly one nixfied input url assignment in flake.nix, found $matches"
          echo "No files were changed."
          echo "Refusing to guess which input pin to rewrite."
        } >&2
        exit 3
      fi

      mv "$rewritten" "$flake"
      changed="flake.nix (nixfied.url -> $nixfied_url)"
    fi

    if [[ "$update_lock" -eq 1 ]]; then
      if command -v nix >/dev/null 2>&1; then
        if nix flake update nixfied --flake "$root" >/dev/null 2>&1; then
          changed="''${changed:+$changed; }flake.lock (nixfied input)"
        else
          echo "warning: failed to refresh flake.lock for the nixfied input; run 'nix flake update nixfied' manually" >&2
        fi
      else
        echo "warning: nix not found on PATH; skipped flake.lock refresh" >&2
      fi
    fi

    if [[ -z "$changed" ]]; then
      echo "Nothing to upgrade in $root"
      echo "Pass --nixfied-url to repin the input, or drop --no-lock to refresh the lock."
      exit 0
    fi

    echo "Upgraded Nixfied wiring in $root"
    echo "changed: $changed"
    if [[ -n "$preserved" ]]; then
      echo "preserved (project-owned): $preserved"
    fi
    echo "next: nix build .#model"
  '';
}
