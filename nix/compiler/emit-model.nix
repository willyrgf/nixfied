{ pkgs, derived }:

let
  modelJson = builtins.toJSON derived.model;
in
pkgs.runCommand "nixfied-model"
  {
    m0Helper = derived.package;
    passAsFile = [ "modelJson" ];
    inherit modelJson;
  }
  ''
    mkdir -p "$out"
    cp "$modelJsonPath" "$out/model.json"
  ''
