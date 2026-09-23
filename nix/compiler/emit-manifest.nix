{ pkgs, derived }:

let
  manifestJson = builtins.toJSON derived.manifest;
  docsMarkdown = import ./views.nix { manifest = derived.manifest; };
in
pkgs.runCommand "nixfied-manifest"
  {
    # Keep every realised closure in the build closure; the runtime requires
    # each referenced store path to already exist before it starts.
    buildInputs = derived.packages;
    passAsFile = [
      "manifestJson"
      "docsMarkdown"
    ];
    inherit manifestJson docsMarkdown;
  }
  ''
    mkdir -p "$out"
    mkdir -p "$out/views"
    cp "$manifestJsonPath" "$out/manifest.json"
    cp "$docsMarkdownPath" "$out/views/docs.md"
  ''
