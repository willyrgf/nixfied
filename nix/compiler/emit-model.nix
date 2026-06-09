{ pkgs, derived }:

let
  modelJson = builtins.toJSON derived.model;
  views = import ./views.nix { model = derived.model; };
  schemaJson = builtins.toJSON views.schema;
  capabilitiesJson = builtins.toJSON views.capabilities;
in
pkgs.runCommand "nixfied-model"
  {
    # Keep every realised closure in the build closure; the runtime requires
    # each referenced store path to already exist before it starts.
    buildInputs = derived.packages;
    passAsFile = [
      "modelJson"
      "schemaJson"
      "capabilitiesJson"
      "docsMarkdown"
    ];
    inherit
      modelJson
      schemaJson
      capabilitiesJson
      ;
    docsMarkdown = views.docs;
  }
  ''
    mkdir -p "$out"
    mkdir -p "$out/views"
    cp "$modelJsonPath" "$out/model.json"
    cp "$schemaJsonPath" "$out/views/schema.json"
    cp "$capabilitiesJsonPath" "$out/views/capabilities.json"
    cp "$docsMarkdownPath" "$out/views/docs.md"
  ''
