{
  pkgs,
  system,
  projectRoot,
  projectModules,
  extraModules ? [ ],
  localOverrides ? [ ],
  frameworkSourceRevision ? import ./framework-revision.nix {
    sourcePath = ../../.;
    metadataPath = ../../VENDORED.txt;
  },
}:
let
  lib = pkgs.lib;
  mkShellApp = import ./mk-shell-app.nix { inherit pkgs; };
  canonical = import ./canonical.nix { inherit lib; };
  kernelPackage = import ../runtime/kernel { inherit pkgs; };

  frameworkRoot = ../../.;
  frameworkRepoRoot = ../../../.;
  frameworkUtilityCommand = import ../install/wrapper-command.nix {
    inherit
      pkgs
      frameworkSourceRevision
      ;
    sourceRoot = frameworkRoot;
    repoRoot = frameworkRepoRoot;
  };

  compiledCore = import ./mkCompiledCore.nix {
    inherit
      pkgs
      system
      projectRoot
      projectModules
      extraModules
      localOverrides
      frameworkSourceRevision
      ;
  };

  validationIrAsset = pkgs.writeText "nixfied-validation-ir.json" ''
    ${builtins.toJSON compiledCore.validationIr}
  '';

  coreSurfaces = import ./mkCoreSurfaces.nix {
    inherit
      pkgs
      canonical
      ;
    compiledCore = compiledCore;
  };

  compiledExecution = (compiledCore.model.compiled or { }).execution or { };
  taskHelpSupport = import ./mkTaskHelpFiles.nix {
    inherit
      pkgs
      compiledExecution
      ;
  };

  runtimeArtifacts = import ./materializeExecution.nix {
    inherit
      pkgs
      projectRoot
      ;
    compiledCore = compiledCore;
  };

  runtimeApps = import ./mkRuntimeAppSet.nix {
    inherit pkgs;
    runtimeProgram = "${runtimeArtifacts.runtimeEngine}/bin/nixfied-runtime";
    model = compiledCore.model;
    contractBundle = compiledCore.contractBundle;
  };

  frameworkUtilityApps = {
    "framework::install" = mkShellApp {
      appName = "framework::install";
      binPrefix = "nixfied-framework";
      body = ''
        if [ "$#" -gt 0 ]; then
          case "$1" in
            --help|-h)
              cat ${lib.escapeShellArg (taskHelpSupport.taskHelpFileFor "task.framework.install")}
              exit 0
              ;;
          esac
        fi

        ${frameworkUtilityCommand { }}
      '';
    };

    "framework::upgrade" = mkShellApp {
      appName = "framework::upgrade";
      binPrefix = "nixfied-framework";
      body = ''
        if [ "$#" -gt 0 ]; then
          case "$1" in
            --help|-h)
              cat ${lib.escapeShellArg (taskHelpSupport.taskHelpFileFor "task.framework.upgrade")}
              exit 0
              ;;
          esac
        fi

        ${frameworkUtilityCommand {
          upgradeDefault = true;
        }}
      '';
    };
  };

  apps =
    runtimeApps
    // coreSurfaces.apps
    // frameworkUtilityApps
    // {
      default = coreSurfaces.apps.help;
    };

  packages = coreSurfaces.packages // {
    default = pkgs.runCommand "nixfied-default" { } ''
      mkdir -p "$out/bin"
      ln -s ${apps.default.program} "$out/bin/default"
    '';
    "nixfied-kernel" = kernelPackage;
    "validation-ir" = validationIrAsset;
  };
in
{
  model = compiledCore.model;
  stateHash = compiledCore.stateHash;
  runtimeHash = runtimeArtifacts.runtimeHash or compiledCore.model.identity.evalHash;
  tasks = compiledCore.model.tasks;
  services = runtimeArtifacts.services;
  serviceDefinitions = compiledCore.resolved.services or { };
  serviceCatalog = compiledCore.model.serviceCatalog;
  workflows = compiledCore.model.workflows;
  features = compiledCore.model.features;
  serviceSurfaceCatalog = (compiledCore.model.compiled or { }).serviceSurfaceCatalog or { };
  serviceApis =
    ((compiledCore.model.compiled or { }).serviceSurfaceCatalog or { }).serviceApis or { };
  inherit
    packages
    apps
    ;
  validationIr = compiledCore.validationIr;
  checks = coreSurfaces.checks;
  devShells = coreSurfaces.devShells;
  schema = coreSurfaces.schema;
  legacyPackages = { };
}
