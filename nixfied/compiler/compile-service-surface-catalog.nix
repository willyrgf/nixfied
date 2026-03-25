{
  lib,
  pkgs,
}:
{
  resolvedIdentity,
  runtime,
  statePolicy,
  services,
}:
let
  serviceModulePath = import ../framework/core/serviceModulePath.nix;

  normalizeToken =
    value: lib.toUpper (lib.replaceStrings [ "." "-" ":" "/" " " ] [ "_" "_" "_" "_" "_" ] value);

  normalizeHookName =
    serviceName: opName: opCfg:
    let
      prefix = normalizeToken serviceName;
      suffix = normalizeToken (if (opCfg.hook or "") != "" then opCfg.hook else opName);
    in
    "SVC_${prefix}_${suffix}";

  enabledServiceIds = builtins.sort builtins.lessThan (
    builtins.filter (serviceId: services.${serviceId}.enable or false) (builtins.attrNames services)
  );

  slotInfo = pkgs.writeShellScript "service-surface-slot-info" ''
    printf 'SLOT=%q\n' "0"
    printf 'ENV=%q\n' ${lib.escapeShellArg runtime.env.default}
    printf 'RUN_DIR=%q\n' "/tmp"
    printf 'LOG_DIR=%q\n' "/tmp"
    printf 'CONFIG_DIR=%q\n' "/tmp"
  '';

  slotInfoJson = pkgs.writeShellScript "service-surface-slot-info-json" ''
    printf '{"slot":"0","env":"%s","ports":{},"directories":{"run":"/tmp","log":"/tmp","config":"/tmp"}}\n' \
      ${lib.escapeShellArg runtime.env.default}
  '';

  serviceProject = {
    project = {
      id = resolvedIdentity.projectId;
      slotVar = runtime.slot.var;
      envVar = runtime.env.var;
    };
    logging = {
      level = runtime.logging.levelDefault;
      output = runtime.logging.outputDefault;
    };
    state = {
      policy = statePolicy;
    };
    ci = {
      artifacts = {
        dir = statePolicy.artifactsRoot;
      };
    };
    directories = {
      base = runtime.directories.base;
    };
    inherit services;
  };

  slots = {
    getSlotInfo = slotInfo;
    getSlotInfoJson = slotInfoJson;
    portVarName = portKey: "${normalizeToken portKey}_PORT";
    getServiceDir = dataDirName: "\${NIXFIED_SERVICE_ROOT}/${dataDirName}";
  };

  runtimeHelpers = import ../framework/runtime/helpers/default.nix {
    inherit pkgs;
    project = serviceProject;
    hooks = { };
  };

  serviceModules = builtins.listToAttrs (
    map (
      serviceId:
      let
        service = services.${serviceId};
        serviceName = service.name or (lib.removePrefix "service." serviceId);
      in
      {
        name = serviceName;
        value = import (serviceModulePath serviceName) {
          inherit
            pkgs
            slots
            ;
          project = serviceProject;
        };
      }
    ) enabledServiceIds
  );

  serviceApis = runtimeHelpers.serviceApi.validateServiceApis (
    runtimeHelpers.serviceApi.mkServiceApisFromModules serviceModules
  );

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
    serviceName: api: opName:
    let
      opCfg = api.operations.${opName};
      appName = if (opCfg.appName or "") != "" then opCfg.appName else "svc::${serviceName}::${opName}";
      includeApp = opCfg.exposeApp or true;
      includeHook = opCfg.exposeHook or true;
      usage = opCfg.usage or [ "nix run .#${appName}" ];
      examples = opCfg.examples or [ ];
      category = opCfg.category or serviceName;
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
      ownerFile = builtins.toString (serviceModulePath serviceName);
      artifacts = api.artifacts or { };
      profiles = api.profiles or [ ];
      runtimePrimitives = api.runtimePrimitives or { };
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
    serviceName: api:
    builtins.listToAttrs (
      map (opName: {
        name = opName;
        value = mkOperationRecord serviceName api opName;
      }) (builtins.sort builtins.lessThan (builtins.attrNames (api.operations or { })))
    )
  ) serviceApis;

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
  inherit
    serviceApis
    appServiceByName
    appsByName
    operationCatalog
    ;
}
