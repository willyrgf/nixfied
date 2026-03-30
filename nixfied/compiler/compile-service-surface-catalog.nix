{
  lib,
  pkgs,
}:
{
  services,
  serviceDefinitions,
}:
let
  serviceContractValidation = import ../framework/core/service-contract-validation.nix {
    inherit pkgs;
  };
  tokenLib = import ../framework/runtime/helpers/normalize-token.nix { inherit lib; };
  normalizeToken = tokenLib.normalizeToken;

  normalizeHookName =
    serviceName: opName: opCfg:
    let
      prefix = normalizeToken serviceName;
      suffix =
        if (opCfg.hook or null) != null && (opCfg.hook or "") != "" then
          opCfg.hook
        else
          normalizeToken opName;
    in
    "SVC_${prefix}_${suffix}";

  enabledServiceIds = builtins.sort builtins.lessThan (
    builtins.filter (serviceId: services.${serviceId}.enable or false) (builtins.attrNames services)
  );

  serviceContracts = builtins.listToAttrs (
    map (
      serviceId:
      let
        service = services.${serviceId};
        serviceName = service.name or (lib.removePrefix "service." serviceId);
        serviceDefinition =
          if builtins.hasAttr serviceName serviceDefinitions then
            serviceDefinitions.${serviceName}
          else
            throw "nixfied service surface catalog: missing typed service definition for '${serviceName}'";
        contract = serviceDefinition.contract or null;
      in
      {
        name = serviceName;
        value =
          if contract == null then
            throw "nixfied service surface catalog: service '${serviceName}' is missing typed contract data"
          else
            contract;
      }
    ) enabledServiceIds
  );

  validatedServiceContracts = serviceContractValidation.validateServiceContracts serviceContracts;

  mkCommandApi =
    {
      appName,
      serviceName,
      opName,
      opCfg,
      usage,
      category,
      examples,
    }:
    {
      version = 1;
      commandClass = opCfg.class or "passthrough";
      summary = opCfg.summary;
      details = opCfg.details;
      inherit
        usage
        examples
        category
        ;
      args = opCfg.args or [ ];
      env = opCfg.env or [ ];
      outputs = null;
      behavior = {
        idempotent = opCfg.idempotent or false;
        effects = [ "none" ];
        timeoutSec = 0;
      };
      errors = {
        codes = { };
      };
      service = serviceName;
      operation = opName;
      appName = appName;
    };

  mkOperationRecord =
    serviceName: contract: opName:
    let
      opCfg = contract.operations.${opName};
      appName =
        if (opCfg.appName or null) != null && opCfg.appName != "" then
          opCfg.appName
        else
          "svc::${serviceName}::${opName}";
      includeApp = opCfg.exposeApp or true;
      includeHook = opCfg.exposeHook or true;
      usage = opCfg.usage or [ "nix run .#${appName}" ];
      examples = opCfg.examples or [ ];
      category = if (opCfg.category or "") != "" then opCfg.category else serviceName;
    in
    {
      inherit
        serviceName
        opName
        appName
        includeApp
        includeHook
        usage
        examples
        category
        ;
      hookName = normalizeHookName serviceName opName opCfg;
      class = opCfg.class or "passthrough";
      idempotent = opCfg.idempotent or false;
      summary = opCfg.summary;
      details = opCfg.details;
      args = opCfg.args or [ ];
      env = opCfg.env or [ ];
      ownerFile = contract.ownerFile;
      artifacts = contract.artifacts or { };
      profiles = contract.profiles or [ ];
      runtimePrimitives = contract.runtimePrimitives or { };
      commandApi = mkCommandApi {
        inherit
          appName
          serviceName
          opName
          opCfg
          usage
          category
          examples
          ;
      };
    };

  operationCatalog = builtins.mapAttrs (
    serviceName: contract:
    builtins.listToAttrs (
      map (opName: {
        name = opName;
        value = mkOperationRecord serviceName contract opName;
      }) (builtins.sort builtins.lessThan (builtins.attrNames (contract.operations or { })))
    )
  ) validatedServiceContracts;

  appEntries = builtins.concatLists (
    map (
      serviceName:
      let
        ops = operationCatalog.${serviceName} or { };
      in
      map (opName: ops.${opName}) (
        builtins.filter (opName: (ops.${opName}.includeApp or false)) (
          builtins.sort builtins.lessThan (builtins.attrNames ops)
        )
      )
    ) (builtins.sort builtins.lessThan (builtins.attrNames operationCatalog))
  );

  appsByName = builtins.listToAttrs (
    map (entry: {
      name = entry.appName;
      value = {
        id = entry.appName;
        kind = "serviceOp";
        service = entry.serviceName;
        operation = entry.opName;
        summary = entry.summary;
        description = entry.details;
        category = entry.category;
        usage = entry.usage;
        examples = entry.examples;
        ownerFile = entry.ownerFile;
        commandApi = entry.commandApi;
      };
    }) appEntries
  );

  appServiceByName = builtins.listToAttrs (
    map (entry: {
      name = entry.appName;
      value = entry.serviceName;
    }) appEntries
  );
in
{
  appNames = builtins.sort builtins.lessThan (builtins.attrNames appServiceByName);
  serviceApis = validatedServiceContracts;
  inherit
    appServiceByName
    appsByName
    operationCatalog
    ;
}
