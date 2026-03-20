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
    localOverrides = [ ./launcher-disabled-nginx-override.nix ];
  };

  serviceHookNames = builtins.sort builtins.lessThan (builtins.attrNames compiled.serviceHookEnv);
  appNames = builtins.sort builtins.lessThan (builtins.attrNames compiled.apps);
in
assert !(builtins.any (hookName: lib.hasPrefix "SVC_NGINX_" hookName) serviceHookNames);
assert !(builtins.any (appName: lib.hasPrefix "svc::nginx::" appName) appNames);
pkgs.runCommand "disabled-service-runtime-surface-smoke" { } ''
  echo "OK: disabled services do not generate runtime hook env or service apps" > "$out"
''
