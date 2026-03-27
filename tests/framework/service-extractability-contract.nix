{ pkgs }:
let
  lib = pkgs.lib;
  project = {
    install.deps = "";
    tooling.runtimePackages = [ ];
    logging = {
      level = "info";
      output = "stdout";
    };
  };
  runtimeHelpers = import ../../nixfied/framework/runtime/helpers/default.nix {
    inherit
      pkgs
      project
      ;
    hooks = { };
  };
  serviceApi = runtimeHelpers.serviceApi;

  mkOpScript =
    serviceName: opName:
    pkgs.writeShellScript "service-extractability-${serviceName}-${opName}" ''
      exit 0
    '';

  mkFixtureApi =
    serviceName:
    serviceApi.mkServiceApiV3 {
      service = serviceName;
      summary = "Fixture ${serviceName} service";
      details = "Fixture service used to prove publicApi extractability.";
      artifacts = {
        endpoint = "/tmp/${serviceName}.sock";
      };
      runtimePrimitives = serviceApi.mkRuntimePrimitivesV1 {
        logLevelDefault = "debug";
        outputModeDefault = "stdout";
      };
      operations = {
        start = {
          summary = "Start ${serviceName}";
          details = "Start fixture service ${serviceName}.";
          script = mkOpScript serviceName "start";
        };
        stop = {
          summary = "Stop ${serviceName}";
          details = "Stop fixture service ${serviceName}.";
          script = mkOpScript serviceName "stop";
        };
        status = {
          summary = "Status ${serviceName}";
          details = "Inspect fixture service ${serviceName}.";
          script = mkOpScript serviceName "status";
        };
        sync = {
          summary = "Sync ${serviceName}";
          details = "Sync fixture state for ${serviceName}.";
          script = mkOpScript serviceName "sync";
          exposeApp = false;
          hook = "sync";
        };
        describe = {
          summary = "Describe ${serviceName}";
          details = "Describe fixture service ${serviceName}.";
          script = mkOpScript serviceName "describe";
          exposeHook = false;
          appName = "svc::${serviceName}::describe";
        };
      };
    };

  fixtureModules = {
    alpha = {
      publicApi = mkFixtureApi "alpha";
      config = throw "private touched: alpha.config";
      exportedImpl = throw "private touched: alpha.exportedImpl";
    };
    beta = {
      publicApi = mkFixtureApi "beta";
      config = throw "private touched: beta.config";
      exportedImpl = throw "private touched: beta.exportedImpl";
    };
  };

  serviceApis = serviceApi.validateEnabledServicesHaveContracts {
    enabledServices = [
      "alpha"
      "beta"
    ];
    serviceApis = serviceApi.mkServiceApisFromModules fixtureModules;
  };
  hookEnv = serviceApi.mkServiceHookEnvFromContract serviceApis;
  serviceApps = serviceApi.mkServiceAppsFromContract serviceApis;

  expectedHookNames = [
    "SVC_ALPHA_START"
    "SVC_ALPHA_STATUS"
    "SVC_ALPHA_STOP"
    "SVC_ALPHA_sync"
    "SVC_BETA_START"
    "SVC_BETA_STATUS"
    "SVC_BETA_STOP"
    "SVC_BETA_sync"
  ];
  expectedAppNames = [
    "svc::alpha::describe"
    "svc::alpha::start"
    "svc::alpha::status"
    "svc::alpha::stop"
    "svc::beta::describe"
    "svc::beta::start"
    "svc::beta::status"
    "svc::beta::stop"
  ];
  renderedAppNames = builtins.sort builtins.lessThan (builtins.attrNames serviceApps);
  renderedHookNames = builtins.sort builtins.lessThan (builtins.attrNames hookEnv);
in
assert (
  builtins.sort builtins.lessThan (builtins.attrNames serviceApis) == [
    "alpha"
    "beta"
  ]
);
assert (
  builtins.sort builtins.lessThan (builtins.attrNames serviceApis.alpha.operations) == [
    "describe"
    "start"
    "status"
    "stop"
    "sync"
  ]
);
assert (
  builtins.sort builtins.lessThan (builtins.attrNames serviceApis.beta.operations) == [
    "describe"
    "start"
    "status"
    "stop"
    "sync"
  ]
);
assert renderedHookNames == expectedHookNames;
assert renderedAppNames == expectedAppNames;
assert serviceApps."svc::alpha::start".meta.nixfied.service == "alpha";
assert serviceApps."svc::alpha::start".meta.nixfied.operation == "start";
assert serviceApps."svc::beta::describe".meta.nixfied.service == "beta";
assert serviceApps."svc::beta::describe".meta.nixfied.operation == "describe";
assert !(builtins.hasAttr "svc::alpha::sync" serviceApps);
assert !(builtins.hasAttr "svc::beta::sync" serviceApps);
assert !(builtins.hasAttr "SVC_ALPHA_DESCRIBE" hookEnv);
assert !(builtins.hasAttr "SVC_BETA_DESCRIBE" hookEnv);
assert lib.all (hookName: hookEnv.${hookName} != "") expectedHookNames;
pkgs.runCommand "service-extractability-contract" { } ''
  echo "OK: framework service helpers consume declared publicApi surfaces without touching private module attrs" > "$out"
''
