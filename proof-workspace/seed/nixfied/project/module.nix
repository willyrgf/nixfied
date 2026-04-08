{
  lib,
  pkgs,
  ...
}:
let
  conf = import ./conf.nix { inherit pkgs; };
  project = conf.project;
  envNames = builtins.attrNames conf.envs;
  envOffsets = lib.mapAttrs (_: value: value.offset or 0) conf.envs;
  commonRuntimeInputs = conf.tooling.runtimePackages or [ ];

  workspaceStatePolicyModule = {
    config.nixfied.state.policy = {
      id = "workspace-scoped";
      kind = "workspace-scoped";
      source = "proof-workspace/seed/nixfied/project/module.nix";
      ownerScope = "workspace";
      discoveryScope = "workspace";
      workspace.mode = "project-root-hash";
      workspace.hashLength = 12;
      roots = {
        runtimeBase = "\${NIX_BUILD_TOP:-\${XDG_CACHE_HOME:-$HOME/.cache}}/nixfied-runtime/${project.id}/{workspaceId}/runtime";
        registryRoot = "\${NIX_BUILD_TOP:-\${XDG_CACHE_HOME:-$HOME/.cache}}/nixfied-runtime/${project.id}/{workspaceId}/registry";
        artifactsRoot = "/tmp/nixfied-artifacts-${project.id}-{workspaceId}";
      };
    };
  };

  projectRuntimeModule = import ./runtime.nix {
    inherit
      conf
      envNames
      envOffsets
      ;
  };

  projectServicesModule = import ./services.nix {
    inherit
      lib
      pkgs
      ;
    inherit conf;
  };

  projectTasksModule = import ./tasks.nix {
    inherit
      lib
      pkgs
      conf
      commonRuntimeInputs
      ;
  };

  projectWorkflowsModule = import ./workflows.nix;
in
{
  imports = [
    workspaceStatePolicyModule
    projectRuntimeModule
    projectServicesModule
    projectTasksModule
    projectWorkflowsModule
  ];
}
