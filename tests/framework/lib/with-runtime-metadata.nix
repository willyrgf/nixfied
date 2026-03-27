{ pkgs }:
model:
let
  lib = pkgs.lib;
  selectionIndex = import ../../../nixfied/compiler/compile-selection-index.nix { inherit lib; } {
    tasks = model.tasks or { };
    workflows = model.workflows or { };
    serviceCatalog = model.serviceCatalog or { };
  };
  runtimeMetadata = import ../../../nixfied/compiler/compile-runtime-metadata.nix { inherit lib; } {
    tasks = model.tasks or { };
    workflows = model.workflows or { };
    serviceCatalog = model.serviceCatalog or { };
    selectionIndex = selectionIndex;
  };
in
model
// {
  compiled = (model.compiled or { }) // {
    inherit runtimeMetadata;
  };
}
