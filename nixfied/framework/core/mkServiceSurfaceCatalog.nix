{
  pkgs,
  model,
}:
let
  lib = pkgs.lib;
  serviceApi = import ../runtime/helpers/service-api.nix { inherit pkgs; };
  serviceModulePath = import ./serviceModulePath.nix;

  normalizeToken =
    value: lib.toUpper (lib.replaceStrings [ "." "-" ":" "/" " " ] [ "_" "_" "_" "_" "_" ] value);

  serviceCatalog = model.serviceCatalog or { };

  enabledServiceNames = builtins.sort builtins.lessThan (
    lib.unique (
      builtins.concatLists (
        map (
          serviceId:
          let
            service = serviceCatalog.${serviceId};
          in
          lib.optionals (service.enable or false) [ service.name or serviceId ]
        ) (builtins.attrNames serviceCatalog)
      )
    )
  );

  stubProject = {
    project = {
      id = model.identity.projectId or "nixfied";
      slotVar = model.runtime.slot.var or "NIX_ENV";
      envVar = model.runtime.env.var or "PROJECT_ENV";
    };
    logging = {
      level = model.runtime.logging.levelDefault or "info";
      output = model.runtime.logging.outputDefault or "stdout";
    };
    state = {
      registry.root = model.state.registry.root or "/tmp/nixfied-runtime";
      artifacts.root = model.state.artifacts.root or "/tmp/ci-artifacts";
    };
    ci.artifacts.dir = model.state.artifacts.root or "/tmp/ci-artifacts";
    directories.base = model.runtime.directories.base or "/tmp/nixfied-runtime";
    services = { };
  };

  slotInfoStub = pkgs.writeShellScript "nixfied-slot-info-stub" ''
    set -euo pipefail
    echo "PROJECT_ENV=${lib.escapeShellArg (model.runtime.env.default or "dev")}"
    echo "NIX_ENV=${lib.escapeShellArg (toString (model.runtime.slot.default or 0))}"
  '';

  slotInfoJsonStub = pkgs.writeShellScript "nixfied-slot-info-json-stub" ''
    set -euo pipefail
    echo '{}'
  '';

  stubSlots = {
    getSlotInfo = slotInfoStub;
    getSlotInfoJson = slotInfoJsonStub;
    portVarName = portKey: "${normalizeToken portKey}_PORT";
    getServiceDir = dataDirName: "/tmp/nixfied-surface/${dataDirName}";
  };

  serviceModules = builtins.listToAttrs (
    map (serviceName: {
      name = serviceName;
      value = import (serviceModulePath serviceName) {
        inherit
          pkgs
          ;
        project = stubProject;
        slots = stubSlots;
      };
    }) enabledServiceNames
  );

  serviceApis = serviceApi.mkServiceApisFromModules serviceModules;
  ops = serviceApi.collectServiceOps serviceApis;
  appOps = builtins.filter (op: op.includeApp) ops;

  appNames = builtins.sort builtins.lessThan (lib.unique (map (op: op.appName) appOps));

  appServiceByName = builtins.listToAttrs (
    map (op: {
      name = op.appName;
      value = op.serviceName;
    }) appOps
  );
in
{
  inherit
    appNames
    appServiceByName
    ;
}
