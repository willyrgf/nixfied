{ pkgs, derived }:

let
  modelJson = builtins.toJSON derived.model;
  docsMarkdown = import ./views.nix { model = derived.model; };
in
pkgs.runCommand "nixfied-model"
  {
    # Keep every realised closure in the build closure; the runtime requires
    # each referenced store path to already exist before it starts.
    buildInputs = derived.packages;
    passAsFile = [
      "modelJson"
      "docsMarkdown"
    ];
    inherit modelJson docsMarkdown;
  }
  ''
    mkdir -p "$out"
    mkdir -p "$out/views"
    cp "$modelJsonPath" "$out/model.json"
    cp "$docsMarkdownPath" "$out/views/docs.md"
  ''
