{
  description = "Nixfied framework";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs?ref=nixos-unstable";
    flake-utils.url = "github:numtide/flake-utils";
  };

  outputs =
    {
      self,
      nixpkgs,
      flake-utils,
      ...
    }:
    let
      supportedSystems = flake-utils.lib.defaultSystems;
      canonicalLib = import ./nixfied/framework/core/canonical.nix { lib = nixpkgs.lib; };
      contractsLib = import ./nixfied/contracts {
        lib = nixpkgs.lib;
        canonical = canonicalLib;
      };

      frameworkRevision = import ./nixfied/framework/core/framework-revision.nix {
        inherit self;
        sourcePath = ./.;
        metadataPath = ./nixfied/VENDORED.txt;
      };

      mkForSystem =
        system:
        let
          pkgs = import nixpkgs { inherit system; };
          frameworkLib = import ./nixfied/framework/core {
            inherit
              pkgs
              system
              ;
          };

          frameworkOutputs = frameworkLib.mkFlakeOutputs {
            projectRoot = ./.;
            projectModules = [ ./nixfied/project/module.nix ];
            extraModules = [ ./nixfied/framework/testing/repo-overlay.nix ];
            localOverrides = [ ];
            frameworkSourceRevision = frameworkRevision;
          };

          frameworkChecks = import ./tests/framework {
            inherit
              pkgs
              ;
            model = frameworkOutputs.model;
            services = frameworkOutputs.services;
            serviceDefinitions = frameworkOutputs.serviceDefinitions;
            serviceCatalog = frameworkOutputs.serviceCatalog;
            apps = frameworkOutputs.apps;
            packages = frameworkOutputs.packages;
            stateHash = frameworkOutputs.stateHash;
            canonical = frameworkLib.canonical;
            registry = import ./nixfied/framework/runtime/registry {
              inherit
                pkgs
                ;
            };
          };
        in
        {
          apps = frameworkOutputs.apps;
          packages = frameworkOutputs.packages;
          legacyPackages = frameworkOutputs.legacyPackages;
          checks = frameworkChecks;
          devShells = frameworkOutputs.devShells;
        };
    in
    (flake-utils.lib.eachSystem supportedSystems mkForSystem)
    // {
      lib = {
        mkNixfied =
          {
            system,
            projectRoot,
            projectModules,
            extraModules ? [ ],
            localOverrides ? [ ],
            frameworkSourceRevision ? frameworkRevision,
          }:
          let
            pkgs = import nixpkgs { inherit system; };
            frameworkLib = import ./nixfied/framework/core {
              inherit
                pkgs
                system
                ;
            };
          in
          frameworkLib.mkNixfied {
            inherit
              projectRoot
              projectModules
              extraModules
              localOverrides
              frameworkSourceRevision
              ;
          };

        mkFlakeOutputs =
          {
            system,
            projectRoot,
            projectModules,
            extraModules ? [ ],
            localOverrides ? [ ],
            frameworkSourceRevision ? frameworkRevision,
          }:
          let
            pkgs = import nixpkgs { inherit system; };
            frameworkLib = import ./nixfied/framework/core {
              inherit
                pkgs
                system
                ;
            };
          in
          frameworkLib.mkFlakeOutputs {
            inherit
              projectRoot
              projectModules
              extraModules
              localOverrides
              frameworkSourceRevision
              ;
          };

        canonical = import ./nixfied/framework/core/canonical.nix { lib = nixpkgs.lib; };
        contracts = contractsLib;
      };

      nixfied = {
        modules = import ./nixfied/modules;
        contracts = contractsLib;
        schemas = {
          task = ./nixfied/schemas/task-contract.json;
          workflow = ./nixfied/schemas/workflow-contract.json;
          model = ./nixfied/schemas/model-export.json;
        };
      };
    };
}
