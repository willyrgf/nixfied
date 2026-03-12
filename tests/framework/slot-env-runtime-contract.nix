{ pkgs }:
let
  source = builtins.readFile ../../nixfied/framework/runtime/helpers/slot-env-runtime.nix;
in
assert pkgs.lib.hasInfix "def emit($name; $value):" source;
assert pkgs.lib.hasInfix "map(\"export \" + .key + \"=\" + (.value | tostring | @sh))" source;
assert !(pkgs.lib.hasInfix "@base64" source);
pkgs.runCommand "slot-env-runtime-contract" { } ''
  echo "OK: slot env runtime uses compiled shell assignments" > "$out"
''
