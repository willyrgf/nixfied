{
  pkgs,
  contractBundle,
  contractRef,
}:
let
  lib = pkgs.lib;
  canonical = import ../core/canonical.nix { inherit lib; };
  contracts = import ../../contracts {
    inherit
      lib
      canonical
      ;
  };
  cueSelector = "#${contracts.types.toCueIdentifier contractRef}";
  cueBundle = contractBundle.cueBundle or (throw "mkValidator: contract bundle is missing cueBundle");
  cueFile = pkgs.writeText "nixfied-contract-${builtins.substring 0 10 (builtins.hashString "sha256" contractRef)}.cue" cueBundle;
in
if !(builtins.hasAttr contractRef (contractBundle.bundle.definitions or { })) then
  throw "mkValidator: unknown contractRef '${contractRef}'"
else
  pkgs.writeShellScript "nixfied-contract-validate-${builtins.substring 0 10 (builtins.hashString "sha256" contractRef)}" ''
    set -euo pipefail

    if [ "$#" -ne 1 ]; then
      echo "usage: nixfied-contract-validate <json-file>" >&2
      exit 1
    fi

    payload_file="$1"
    payload_json="$(mktemp "''${TMPDIR:-/tmp}/nixfied-contract-payload.XXXXXX.json")"
    cleanup_payload_json() {
      rm -f "$payload_json"
    }
    trap cleanup_payload_json EXIT

    cp "$payload_file" "$payload_json"
    exec ${pkgs.cue}/bin/cue vet -c ${lib.escapeShellArg cueFile} "$payload_json" -d ${lib.escapeShellArg cueSelector}
  ''
