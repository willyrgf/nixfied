_:
let
  compilePolicy =
    {
      projectRoot,
      identity,
      policy,
    }:
    let
      workspaceId =
        if policy.workspace.mode == "literal" then
          if (policy.workspace.value or null) == null || policy.workspace.value == "" then
            throw "ERROR: nixfied.state.policy.workspace.value must be set when workspace.mode = literal"
          else
            policy.workspace.value
        else
          builtins.substring 0 policy.workspace.hashLength (
            builtins.hashString "sha256" (toString projectRoot)
          );

      replaceTokens =
        template:
        builtins.replaceStrings
          [ "{projectId}" "{workspaceId}" ]
          [
            identity.projectId
            workspaceId
          ]
          template;
    in
    {
      inherit (policy) id;
      inherit (policy) kind;
      inherit (policy) source;
      inherit (policy) ownerScope;
      inherit (policy) discoveryScope;
      workspace = {
        inherit (policy.workspace) mode;
        inherit (policy.workspace) hashLength;
        value = policy.workspace.value or null;
      };
      roots = {
        runtimeBaseTemplate = policy.roots.runtimeBase;
        registryRootTemplate = policy.roots.registryRoot;
        artifactsRootTemplate = policy.roots.artifactsRoot;
      };
      inherit workspaceId;
      runtimeBase = replaceTokens policy.roots.runtimeBase;
      registryRoot = replaceTokens policy.roots.registryRoot;
      artifactsRoot = replaceTokens policy.roots.artifactsRoot;
    };
in
{
  inherit compilePolicy;
}
