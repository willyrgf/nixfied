{ lib, pkgs, ... }:
let
  catalog = import ../../../nixfied/framework/testing/catalog.nix;

  nonEmptyShardNames =
    profileName:
    builtins.filter (
      shardName: (catalog.profileShardChecks.${profileName}.${shardName} or [ ]) != [ ]
    ) catalog.order;

  sanitizeTaskId = taskId: lib.replaceStrings [ "." ] [ "-" ] taskId;

  mkProbeRunner =
    taskId:
    let
      command = "probe-${sanitizeTaskId taskId}";
      package = pkgs.writeShellScriptBin command ''
        set -euo pipefail
        artifacts_dir="''${CI_ARTIFACTS_DIR:-''${CI_ARTIFACTS_ROOT:-$REGISTRY_ROOT/artifacts/manual}}"
        mkdir -p "$artifacts_dir"
        touch "$artifacts_dir/${sanitizeTaskId taskId}.log"
        echo "OK: probe task complete id=${taskId}"
      '';
    in
    {
      type = "derivation";
      inherit
        package
        command
        ;
      workflowId = null;
    };

  profileTaskEntries = builtins.concatLists (
    map (
      profileName:
      map (shardName: {
        name = "${profileName}-${shardName}";
        value.runner = lib.mkForce (mkProbeRunner "task.test.framework.${profileName}.${shardName}");
      }) (nonEmptyShardNames profileName)
    ) catalog.profileNames
  );

  workflowEntries = map (profileName: {
    name = "test-${profileName}";
    value.execution.ephemeral.enable = lib.mkForce false;
  }) catalog.profileNames;
in
{
  nixfied.tasks = builtins.listToAttrs profileTaskEntries // {
    "test-framework-selfhost".runner = lib.mkForce (mkProbeRunner "task.test.framework.selfhost");
  };

  nixfied.workflows = builtins.listToAttrs workflowEntries // {
    "test-framework-selfhost".execution.ephemeral.enable = lib.mkForce false;
  };
}
