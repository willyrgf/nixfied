# Executable shell evidence for runtime tests. Rust supplies data arguments only.
{ pkgs, testChild }:
let
  fixture = pkgs.writeShellScriptBin "nixfied-test-fixture" ''
    set -eu
    mode="''${1:?missing fixture mode}"
    shift
    case "$mode" in
      exit|prepare)
        exec ${testChild}/bin/nixfied-test-child "$mode" "$@"
        ;;
      marker)
        [ "$#" -eq 1 ]
        printf ran > "$1"
        ;;
      exists)
        [ "$#" -eq 1 ]
        test -e "$1"
        ;;
      launch-input)
        [ "$#" -eq 0 ]
        read -r value
        printf '%s:%s:%s' "$$" "$ONLY" "$value"
        printf ran > marker
        ;;
      launch-bytes)
        [ "$#" -eq 1 ]
        printf '%s' "$1"
        printf ran > marker
        ;;
      launch-signal)
        [ "$#" -eq 0 ]
        kill -TERM "$$"
        printf unexpected > marker
        ;;
      round-count|round-replace|round-fail)
        [ "$#" -eq 7 ]
        [ "$1:$2" = "$3:$4" ] || exit 9
        case "$mode" in
          round-count)
            n=0
            if test -e "$5"; then read -r n < "$5"; fi
            printf '%s\n' "$((n + 1))" > "$5"
            ;;
          round-replace)
            exec ${testChild}/bin/nixfied-test-child prepare "$6" "$7"
            ;;
          round-fail) exit 7 ;;
        esac
        ;;
      orphan-member)
        [ "$#" -eq 1 ]
        ${pkgs.coreutils}/bin/sleep 30 &
        echo $! > "$1"
        ;;
      capture-exited)
        [ "$#" -eq 0 ]
        printf x
        ;;
      capture-live)
        [ "$#" -eq 0 ]
        printf x
        exec ${pkgs.coreutils}/bin/sleep 30
        ;;
      pipe-holder)
        [ "$#" -eq 1 ]
        (${pkgs.coreutils}/bin/sleep 2; printf survived > "$1") &
        printf secret
        ;;
      *) echo "unknown runtime fixture mode" >&2; exit 2 ;;
    esac
  '';
  poison = pkgs.writeShellScriptBin "nixfied-test-poison" ''
    set -eu
    : > "''${NIXFIED_TEST_POISON_SENTINEL:?missing poison sentinel}"
    echo 'Nix tool must not be invoked by nixfied-runtime' >&2
    exit 127
  '';
in
pkgs.symlinkJoin {
  name = "nixfied-runtime-test-fixtures";
  paths = [ fixture poison ];
}
