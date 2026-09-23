{ pkgs }:
let
  upgrade = import ../install/upgrade.nix { inherit pkgs; };
in
pkgs.runCommand "nixfied-upgrade-syntax" { nativeBuildInputs = [ pkgs.diffutils ]; } ''
  export LC_ALL=C
  cd "$(mktemp -d)"
  program=${upgrade}/bin/nixfied-upgrade

  # Compare files, not command substitutions: trailing newlines and non-UTF-8
  # bytes are part of the native parser's output contract.
  check() {
    local expected_status="$1"
    shift
    local status=0
    "$program" "$@" >actual.stdout 2>actual.stderr || status=$?
    if [[ "$status" -ne "$expected_status" ]]; then
      printf 'upgrade parser exited %s, expected %s; arguments:' "$status" "$expected_status" >&2
      printf ' %q' "$@" >&2
      printf '\n' >&2
      exit 1
    fi
    cmp expected.stdout actual.stdout
    cmp expected.stderr actual.stderr
  }

  cat ${../fixtures/upgrade-help.txt} >expected.stdout
  : >expected.stderr
  check 0 --help
  check 0 -h
  check 0 --plan --plan --no-lock --no-lock --help

  : >expected.stdout
  for flag in --root --nixfied-url; do
    printf 'missing %s value\n' "$flag" >expected.stderr
    check 2 "$flag"
    for operand in "" --help --value; do
      check 2 "$flag" "$operand" --help
    done
  done

  for token in --root=. positional -- -hh $'\xff'; do
    {
      printf 'unknown upgrade argument: %s\n' "$token"
      cat ${../fixtures/upgrade-help.txt}
    } >expected.stderr
    check 2 "$token" --help
  done

  # These paths stop at the native missing-flake check before any Nix call.
  missing_flake() {
    local root="$1"
    shift
    printf '%s\n' \
      "no Nixfied flake.nix to upgrade in $root" \
      'No files were changed.' \
      "Run 'nixfied install' first to scaffold a project." >expected.stderr
    check 3 "$@"
  }
  missing_flake .
  missing_flake -h --root -h
  missing_flake second --root first --root second
  missing_flake trimmed --root $'trimmed\n\n'
  missing_flake $'bytes-\xff' --root $'bytes-\xff'
  missing_flake . --nixfied-url first --nixfied-url -h

  touch "$out"
''
