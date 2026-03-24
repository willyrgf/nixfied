{ pkgs }:
let
  lib = pkgs.lib;
  frameworkLib = import ../../nixfied/framework/core {
    inherit pkgs;
    system = pkgs.system;
  };
  compiled = frameworkLib.mkNixfied {
    projectRoot = ../..;
    projectModules = [ ../../nixfied/project/module.nix ];
    extraModules = [ ];
    localOverrides = [ ];
  };
  heavyServiceAppNames = builtins.sort builtins.lessThan (
    builtins.filter (appName: lib.hasPrefix "svc::" appName) (builtins.attrNames compiled.apps)
  );
  cheapServiceAppNames = builtins.sort builtins.lessThan (
    compiled.model.compiled.serviceSurfaceCatalog.appNames or [ ]
  );
  mkCompiledCoreSource = builtins.readFile ../../nixfied/framework/core/mkCompiledCore.nix;
in
assert cheapServiceAppNames == heavyServiceAppNames;
assert !(lib.hasInfix "mkServiceSurfaceCatalog.nix" mkCompiledCoreSource);
pkgs.runCommand "service-surface-catalog-contract" { } ''
  echo "OK: cheap service surface discovery matches the materialized service app surface" > "$out"
''
