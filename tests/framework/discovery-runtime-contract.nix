{ pkgs }:
let
  source = builtins.readFile ../../nixfied/framework/runtime/helpers/discovery.nix;
in
assert pkgs.lib.hasInfix "commonRuntimeShell = import ../common-runtime.nix" source;
assert pkgs.lib.hasInfix "render_docs_json() {" source;
assert pkgs.lib.hasInfix "render_components_json() {" source;
assert pkgs.lib.hasInfix "render_command_surfaces_json() {" source;
assert pkgs.lib.hasInfix "render_features_json() {" source;
assert pkgs.lib.hasInfix "render_risk_areas_json() {" source;
assert pkgs.lib.hasInfix "COMPILED_COMMAND_SURFACE_LINES" source;
assert pkgs.lib.hasInfix "COMPILED_FEATURE_LINES" source;
assert (!pkgs.lib.hasInfix "\${pkgs.jq}/bin/jq" source);
pkgs.runCommand "discovery-runtime-contract" { } ''
  echo "OK: discovery helper is jq-free" > "$out"
''
