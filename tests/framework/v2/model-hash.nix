{
  pkgs,
  model,
  stateHash,
  canonical,
}:
let
  recomputed = canonical.hashCanonical model;
in
assert recomputed == stateHash;
pkgs.runCommand "v2-model-hash" { } ''
  echo "OK: model hash is stable" > "$out"
''
