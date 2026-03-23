{ lib, modules }:
{
  pkgs,
  system,
  projectRoot,
  frameworkSourceRevision,
  projectModules,
  extraModules ? [ ],
  localOverrides ? [ ],
}:
let
  toAbsString =
    value:
    if builtins.isPath value then
      builtins.unsafeDiscardStringContext (builtins.toString value)
    else if builtins.isString value && lib.hasPrefix "/" value then
      builtins.unsafeDiscardStringContext value
    else if builtins.isString value && lib.hasPrefix "./" value then
      builtins.unsafeDiscardStringContext "${builtins.toString projectRoot}/${lib.removePrefix "./" value}"
    else if builtins.isString value then
      builtins.unsafeDiscardStringContext "${builtins.toString projectRoot}/${value}"
    else
      "";

  toAbsPath =
    value:
    let
      absString = toAbsString value;
    in
    if absString == "" then
      throw "nixfied local override must be a path or string path"
    else
      /. + absString;

  legacyLocalDefaultRelativePath = "nixfied/local/default.nix";
  legacyLocalDefaultAbsPath = toAbsString "${builtins.toString projectRoot}/${legacyLocalDefaultRelativePath}";

  isLegacyLocalDefault =
    moduleSpec:
    let
      normalized = toAbsString moduleSpec;
    in
    normalized == legacyLocalDefaultAbsPath;

  adaptLegacyLocalDefault =
    moduleSpec:
    let
      conf = import (projectRoot + "/nixfied/project/conf.nix") { inherit pkgs; };
      legacyLocalPath = toAbsPath moduleSpec;
      legacyLocal = import legacyLocalPath {
        inherit
          pkgs
          lib
          ;
        project = conf.project or { };
        slots = conf.slots or null;
        hooks = conf.hooks or null;
        postgres = conf.services.postgres or null;
        nginx = conf.services.nginx or null;
        minio = conf.services.minio or null;
        reth = conf.services.reth or null;
        helios = conf.services.helios or null;
        supervisor = conf.supervisor or null;
        ephemeral = conf.ephemeral or null;
      };
    in
    {
      _file = builtins.toString legacyLocalPath;
      config.nixfied.legacyLocal = {
        apps = legacyLocal.apps or { };
        packages = legacyLocal.packages or { };
        devShells = legacyLocal.devShells or { };
      };
    };

  adaptedLocalOverrides = map (
    moduleSpec: if isLegacyLocalDefault moduleSpec then adaptLegacyLocalDefault moduleSpec else moduleSpec
  ) localOverrides;

  evaluated = lib.evalModules {
    modules = [
      {
        imports = [ modules.core ];
      }
    ]
    ++ projectModules
    ++ extraModules
    ++ adaptedLocalOverrides;

    specialArgs = {
      inherit
        pkgs
        system
        projectRoot
        frameworkSourceRevision
        modules
        ;
    };
  };
in
{
  config = evaluated.config.nixfied;
}
