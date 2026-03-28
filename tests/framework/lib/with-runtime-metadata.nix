{ pkgs }:
model:
let
  lib = pkgs.lib;
  execution =
    import ../../../nixfied/compiler/compile-execution.nix
      {
        inherit lib;
        canonical = import ../../../nixfied/framework/core/canonical.nix { inherit lib; };
      }
      {
        resolvedIdentity = model.identity or { };
        runtime = model.runtime or { };
        state = model.state or { };
        serviceCatalog = model.serviceCatalog or { };
        serviceSets = model.serviceSets or { };
        apps = model.apps or { };
        tasks = model.tasks or { };
        workflows = model.workflows or { };
      };
  runtimeMetadata = import ../../../nixfied/compiler/compile-runtime-metadata.nix { inherit lib; } {
    apps = model.apps or { };
    compiledExecution = execution;
    tasks = model.tasks or { };
    workflows = model.workflows or { };
    serviceCatalog = model.serviceCatalog or { };
  };
in
model
// {
  compiled =
    (builtins.removeAttrs (model.compiled or { }) [
      "execution"
      "runtimeMetadata"
      "runtimeManifests"
    ])
    // {
      inherit execution;
      inherit runtimeMetadata;
    };
}
