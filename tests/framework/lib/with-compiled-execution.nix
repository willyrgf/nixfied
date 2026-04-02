{ pkgs }:
model:
let
  frameworkLib = import ../../../nixfied/framework/core {
    inherit pkgs;
    system = pkgs.system;
  };
  execution = frameworkLib.compileExecution {
    resolvedIdentity = model.identity or { };
    runtime = model.runtime or { };
    state = model.state or { };
    serviceCatalog = model.serviceCatalog or { };
    serviceSets = model.serviceSets or { };
    apps = model.apps or { };
    tasks = model.tasks or { };
    workflows = model.workflows or { };
  };
in
model
// {
  compiled =
    (builtins.removeAttrs (model.compiled or { }) [
      "execution"
      "runtimeManifests"
    ])
    // {
      inherit execution;
    };
}
