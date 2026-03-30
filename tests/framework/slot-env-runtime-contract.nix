{ pkgs }:
let
  source = builtins.readFile ../../nixfied/framework/core/slot-env-runtime.nix;
  surfaceSource = builtins.readFile ../../nixfied/framework/core/mkServiceRuntimeSurfaces.nix;
in
assert !(pkgs.lib.hasInfix "\${pkgs.jq}/bin/jq" source);
assert !(pkgs.lib.hasInfix "\${pkgs.jq}/bin/jq" surfaceSource);
assert pkgs.lib.hasInfix "evalAssignments =" source;
assert pkgs.lib.hasInfix "printf 'SLOT=%q\\n'" surfaceSource;
pkgs.runCommand "slot-env-runtime-contract" { } ''
  echo "OK: slot env runtime and slot-info surfaces are jq-free" > "$out"
''
