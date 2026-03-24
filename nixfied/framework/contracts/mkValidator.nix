{
  pkgs,
  contractBundle,
  contractRef,
}:
let
  lib = pkgs.lib;
  kernelPackage = import ../runtime/kernel {
    inherit pkgs;
  };
  validationBundleFile = pkgs.writeText "nixfied-validation-bundle-${builtins.substring 0 10 (builtins.hashString "sha256" contractRef)}.json" (
    builtins.toJSON contractBundle.bundle
  );
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
    exec ${kernelPackage}/bin/nixfied-kernel validate-payload ${lib.escapeShellArg validationBundleFile} ${lib.escapeShellArg contractRef} "$payload_file"
  ''
