{ pkgs }:
let
  source = builtins.readFile ../../nixfied/runner/env-sandbox.nix;
in
assert pkgs.lib.hasInfix "run_in_sandbox() {" source;
assert pkgs.lib.hasInfix "ERROR: task runtime.workdir=custom but customWorkdir is empty" source;
assert pkgs.lib.hasInfix "ERROR: unknown runtime.workdir '$workdir_kind'" source;
assert pkgs.lib.hasInfix
  "env -i \"PATH=$final_path\" \"LANG=$locale\" \"LC_ALL=$locale\" \"TZ=$timezone\" \"HOME=$home_value\""
  source;
assert pkgs.lib.hasInfix ".runtime.passThroughEnv[]?" source;
assert pkgs.lib.hasInfix ".runtime.env | to_entries[]?" source;
pkgs.runCommand "env-sandbox-contract" { } ''
  echo "OK: sandbox contract markers are stable" > "$out"
''
