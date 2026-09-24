# The generated surface nixfied derives for an adopting project from its
# module (VERB-1, the corrected split of SURFACE-1):
#
# - the **framework project apps**, framework-owned discovery and controls;
# - the **project verbs**, adopter-owned: one app per task id exported in
#   `nixfied.surface.verbs` (`.#check` -> `runtime run --task check`).
#
# Help renders the generated definitions without inspecting the caller flake.
{
  pkgs,
  lib,
  releaseRuntime,
  system,
  manifest,
  config,
  docs,
  publicationTargets,
}:
let
  runtimeBin = "${releaseRuntime}/bin/nixfied-runtime";
  manifestJson = "${manifest}/manifest.json";
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
          ''exec "${runtimeBin}" ${syntax.name} ${syntax.args.manifest.token} "${manifestJson}" ${syntax.args.task.token} "${verb}" "$@"'';
    }) verbIds
  );
  declarations = import ./project-publications.nix {
    inherit
      pkgs
      system
      runtimeBin
      manifestJson
      docs
      ;
    apps =
      builtins.listToAttrs (
        map (entry: {
          name = entry.name;
          value.meta.description = entry.description;
        }) declarations
      )
      // verbApps;
  };
  framework = import ./meta/publications.nix { inherit lib; } {
    targets = publicationTargets;
    inherit declarations;
  };
  apps = framework.project "app" "project" // verbApps;
in
apps
