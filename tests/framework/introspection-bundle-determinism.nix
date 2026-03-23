{ pkgs }:
let
  frameworkLib = import ../../nixfied/framework/core {
    inherit pkgs;
    system = pkgs.system;
  };
  runtimeArtifactContracts = import ../../nixfied/framework/contracts/runtime-artifact-contracts.nix {
    inherit pkgs;
  };
  cueBundleFile = pkgs.writeText "nixfied-runtime-artifact-contracts.cue" runtimeArtifactContracts.cueBundle;
  batchSchemaFile = pkgs.writeText "nixfied-introspection-batch-schema.cue" ''
    package nixfied

    #runtime__introspectionResponse__batch: [...#runtime__introspectionResponse]
  '';

  firstOutputs = frameworkLib.mkFlakeOutputs {
    projectRoot = ../..;
    projectModules = [ ../../nixfied/project/module.nix ];
    extraModules = [ ];
    localOverrides = [ ];
  };

  secondOutputs = frameworkLib.mkFlakeOutputs {
    projectRoot = ../..;
    projectModules = [ ../../nixfied/project/module.nix ];
    extraModules = [ ];
    localOverrides = [ ];
  };
in
pkgs.runCommand "introspection-bundle-determinism" { } ''
  set -euo pipefail

  bundle_one="${firstOutputs.packages.introspectionBundle}"
  bundle_two="${secondOutputs.packages.introspectionBundle}"
  assets_one="${firstOutputs.packages.introspectionAssets}"
  assets_two="${secondOutputs.packages.introspectionAssets}"
  payload_array="$TMPDIR/introspection-assets.json"

  {
    printf '['
    first=1

    while IFS= read -r json_file; do
      if [ "$first" -eq 0 ]; then
        printf ','
      fi

      ${pkgs.coreutils}/bin/cat "$json_file"
      first=0
    done < <(${pkgs.findutils}/bin/find "$assets_one" -type f -name '*.json')

    printf ']'
  } > "$payload_array"

  ${pkgs.cue}/bin/cue vet \
    -c ${cueBundleFile} \
    ${batchSchemaFile} \
    "$payload_array" \
    -d '#runtime__introspectionResponse__batch'

  ${pkgs.diffutils}/bin/cmp "$bundle_one" "$bundle_two"
  ${pkgs.diffutils}/bin/diff -ru "$assets_one" "$assets_two" > "$TMPDIR/introspection-assets.diff"

  echo "OK: introspection bundle assets validate and remain deterministic" > "$out"
''
