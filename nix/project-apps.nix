# The generated surface nixfied derives for an adopting project from its
# module (VERB-1, the corrected split of SURFACE-1):
#
# - the **framework project apps**, framework-owned discovery and controls;
# - the **project verbs**, adopter-owned: one app per task id exported in
#   `nixfied.surface.verbs` (`.#check` -> `runtime run --task check`).
#
# Runtime-backed apps use the model store path baked in at evaluation. The help
# app instead projects final flake metadata through Nix and never enters the
# runtime, so SEAM-1 remains intact.
{
  module,
  pkgs,
  lib,
  releaseRuntime,
  system,
  model,
  config,
  docs,
  publicationTargets,
}:
let
  modulePath = toString module;
  moduleRoot = builtins.dirOf modulePath;
  moduleIsProjectRoot =
    builtins.isPath module
    && builtins.baseNameOf modulePath == "nixfied.nix"
    && builtins.pathExists "${moduleRoot}/flake.nix"
    && builtins.pathExists "${moduleRoot}/flake.lock";
  runtimeBin = "${releaseRuntime}/bin/nixfied-runtime";
  modelJson = "${model}/model.json";
  syntax = (import ./meta/command-default.nix { inherit lib; }).byName.run;
  verbs = config.nixfied.surface.verbs;
  verbIds = builtins.attrNames verbs;
  mkApp =
    name: description: text:
    let
      drv = pkgs.writeShellApplication { inherit name text; };
    in
    {
      type = "app";
      program = "${drv}/bin/${name}";
      meta.description = description;
    };
  # `validate.nix` already proved each verb names a declared task, avoids the
  # framework namespace, and forced every description; this projection just
  # derives the apps.
  verbApps = builtins.listToAttrs (
    map (verb: {
      name = verb;
      value =
        mkApp verb verbs.${verb}
          ''exec "${runtimeBin}" ${syntax.name} ${syntax.args.model.token} "${modelJson}" ${syntax.args.task.token} "${verb}" "$@"'';
    }) verbIds
  );
  framework = import ./meta/publications.nix { inherit lib; } {
    targets = publicationTargets;
    declarations = import ./project-publications.nix {
      inherit
        pkgs
        system
        moduleRoot
        runtimeBin
        modelJson
        docs
        ;
    };
  };
in
assert lib.assertMsg moduleIsProjectRoot
  "lib.projectApps requires project-root flake.nix, flake.lock, and nixfied.nix";
framework.project "app" "project" // verbApps
