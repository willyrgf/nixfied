set -euo pipefail

# Nixfied upgrade surface.
#
# Ownership boundary (RFC v2 "Install & Upgrade Ownership"):
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
      root="$(take_value "$1" "${2-}")"
      shift 2
      ;;
    --nixfied-url)
      nixfied_url="$(take_value "$1" "${2-}")"
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

if ! grep -Eq '(^|[^A-Za-z0-9_.])nixfied\.url[[:space:]]*=' "$flake"; then
  {
    echo "flake.nix in $root has no Nixfied input pin to upgrade"
    echo "No files were changed."
    echo "Expected an 'inputs.nixfied.url = \"...\";' assignment."
  } >&2
  exit 3
fi

nix_escape() {
  local value="$1"
  value="${value//\\/\\\\}"
  value="${value//\"/\\\"}"
  printf '%s' "$value"
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
  : >"$rewritten"
  while IFS= read -r line || [[ -n "$line" ]]; do
    # Compare on a whitespace-stripped form so we match the exact `nixfied.url`
    # assignment regardless of indentation/spacing, then re-emit it canonically.
    stripped="${line//[[:space:]]/}"
    if [[ "$stripped" == 'nixfied.url="'*'";'* ]]; then
      indent="${line%%nixfied.url*}"
      printf '%snixfied.url = "%s";\n' "$indent" "$nixfied_url_escaped" >>"$rewritten"
      matches=$((matches + 1))
    else
      printf '%s\n' "$line" >>"$rewritten"
    fi
  done <"$flake"

  if [[ "$matches" -ne 1 ]]; then
    rm -f "$rewritten"
    {
      echo "expected exactly one 'nixfied.url = \"...\";' line in flake.nix, found $matches"
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
      changed="${changed:+$changed; }flake.lock (nixfied input)"
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
