{
  pkgs,
  model,
  stateHash,
  canonical,
}:
let
  canonicalizedModel = canonical.canonicalize model;
  recomputed = builtins.hashString "sha256" (canonical.toCanonicalNix canonicalizedModel);
in
assert recomputed == stateHash;
pkgs.runCommand "v2-cross-machine-hash" { } ''
  echo "OK: canonical rendering hash is stable" > "$out"
''
