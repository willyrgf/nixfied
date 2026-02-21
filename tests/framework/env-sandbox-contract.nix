{ pkgs }:
let
  source = builtins.readFile ../../nixfied/runner/env-sandbox.nix;
in
assert pkgs.lib.hasInfix "run_in_sandbox_runtime() {" source;
assert pkgs.lib.hasInfix "run_in_sandbox() {" source;
assert pkgs.lib.hasInfix "ERROR: task runtime.workdir=custom but customWorkdir is empty" source;
assert pkgs.lib.hasInfix "ERROR: unknown runtime.workdir '$workdir_kind'" source;
assert pkgs.lib.hasInfix
  "env -i \"PATH=$final_path\" \"LANG=$locale\" \"LC_ALL=$locale\" \"TZ=$timezone\" \"HOME=$home_value\""
  source;
assert pkgs.lib.hasInfix "/usr/bin/xcrun --sdk macosx --show-sdk-path" source;
assert pkgs.lib.hasInfix "env_cmd+=(\"SDKROOT=$host_sdkroot\")" source;
assert pkgs.lib.hasInfix ".passThroughEnv[]?" source;
assert pkgs.lib.hasInfix ".allowSensitivePassThrough // false" source;
assert pkgs.lib.hasInfix "ERROR: sensitive passthrough env blocked name=$pass_name" source;
assert pkgs.lib.hasInfix ".env | to_entries[]?" source;
assert pkgs.lib.hasInfix ".runtimePackages[]?" source;
assert pkgs.lib.hasInfix ".primitives.defs // {}" source;
assert pkgs.lib.hasInfix "NIXFIED_RUNTIME_DIR_BASE" source;
assert pkgs.lib.hasInfix "NIXFIED_SERVICE_" source;
pkgs.runCommand "env-sandbox-contract" { } ''
  echo "OK: sandbox contract markers are stable" > "$out"
''
