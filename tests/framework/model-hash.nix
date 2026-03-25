{
  pkgs,
  model,
  stateHash,
  canonical,
}:
let
  recomputed = canonical.hashCanonical (builtins.removeAttrs model [ "compiled" ]);
in
assert recomputed == stateHash;
pkgs.runCommand "model-hash" { } ''
  echo "OK: model hash is stable" > "$out"
''
