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
    localOverrides = [
      (
        { lib, ... }:
        {
          nixfied.services.helios = {
            enable = lib.mkForce false;
            sourceKeys = lib.mkForce [ "poison" ];
            defaultSource = lib.mkForce "poison";
            sources.poison.packageFactory = ./poison-package.nix;
          };
        }
      )
    ];
  };

  helpProgram = compiled.apps.help.program;
  runTaskProgram = compiled.apps.run-task.program;
  appNames = builtins.sort builtins.lessThan (builtins.attrNames compiled.apps);
  serviceHookNames = builtins.sort builtins.lessThan (builtins.attrNames compiled.serviceHookEnv);
in
assert builtins.isString helpProgram;
assert builtins.isString runTaskProgram;
assert !(builtins.any (appName: lib.hasPrefix "svc::helios::" appName) appNames);
assert !(builtins.any (hookName: lib.hasPrefix "SVC_HELIOS_" hookName) serviceHookNames);
pkgs.runCommand "disabled-service-no-package-resolution-smoke" { } ''
  echo "OK: disabled services do not resolve poisoned packages or emit runtime surfaces" > "$out"
''
