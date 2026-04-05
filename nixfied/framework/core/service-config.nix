{
  lib,
  pkgs ? null,
}:
let
  runtimeDefaults = import ./runtime-defaults.nix;
  listUtils = import ./list-utils.nix;
  uniqueSorted = listUtils.uniqueSorted;

  packagePath =
    {
      discardContext ? false,
      pkg,
    }:
    if pkg == null then
      null
    else
      let
        path = builtins.toString pkg;
      in
      if discardContext then builtins.unsafeDiscardStringContext path else path;

  dropNulls =
    attrs:
    builtins.removeAttrs attrs (
      builtins.filter (name: attrs.${name} == null) (builtins.attrNames attrs)
    );

  hasNonNullAttr = attrs: name: builtins.hasAttr name attrs && builtins.getAttr name attrs != null;

  resolveAttrPath =
    {
      serviceName,
      sourceKey,
      fieldName,
      attrPath,
    }:
    let
      segments = lib.splitString "." attrPath;
      step =
        current: remaining:
        if remaining == [ ] then
          current
        else
          let
            key = builtins.head remaining;
          in
          if !(builtins.isAttrs current) || !(builtins.hasAttr key current) then
            throw ''
              service '${serviceName}' source '${sourceKey}' ${fieldName} '${attrPath}' is not defined in pkgs
            ''
          else
            step (builtins.getAttr key current) (builtins.tail remaining);
    in
    if pkgs == null then null else step pkgs segments;

  resolveSourcePackage =
    {
      serviceName,
      sourceKey,
      source,
      fieldName,
      attrFieldName,
      factoryFieldName,
    }:
    let
      selectorFields = [
        fieldName
        attrFieldName
        factoryFieldName
      ];
      configuredSelectors = builtins.filter (name: hasNonNullAttr source name) selectorFields;
      selectedField = if configuredSelectors == [ ] then null else builtins.head configuredSelectors;
    in
    if builtins.length configuredSelectors > 1 then
      throw ''
        service '${serviceName}' source '${sourceKey}' defines multiple selectors for ${fieldName}: ${builtins.concatStringsSep ", " configuredSelectors}
      ''
    else if selectedField == null then
      null
    else if selectedField == fieldName then
      builtins.getAttr fieldName source
    else if selectedField == attrFieldName then
      resolveAttrPath {
        inherit
          serviceName
          sourceKey
          fieldName
          ;
        attrPath = builtins.getAttr attrFieldName source;
      }
    else if pkgs == null then
      null
    else
      pkgs.callPackage (builtins.getAttr factoryFieldName source) { };

  normalizeSource =
    discardContext: serviceName: sourceKey: source:
    let
      package = resolveSourcePackage {
        inherit
          serviceName
          sourceKey
          source
          ;
        fieldName = "package";
        attrFieldName = "packageAttr";
        factoryFieldName = "packageFactory";
      };
      clientPackage = resolveSourcePackage {
        inherit
          serviceName
          sourceKey
          source
          ;
        fieldName = "clientPackage";
        attrFieldName = "clientPackageAttr";
        factoryFieldName = "clientPackageFactory";
      };
    in
    dropNulls {
      package =
        if package != null then
          packagePath {
            inherit discardContext;
            pkg = package;
          }
        else
          null;
      clientPackage =
        if clientPackage != null then
          packagePath {
            inherit discardContext;
            pkg = clientPackage;
          }
        else
          null;
    };

  sourceKeys = cfg: uniqueSorted ((cfg.sourceKeys or [ ]) ++ builtins.attrNames (cfg.sources or { }));

  resolveSelectedSource =
    serviceName: keys: defaultSource:
    if defaultSource == "" then
      ""
    else if builtins.elem defaultSource keys then
      defaultSource
    else
      throw "service '${serviceName}' defaultSource '${defaultSource}' is not defined in sources";

  normalizeSelectedSources =
    discardContext: serviceName: keys: selectedSource: sources:
    builtins.listToAttrs (
      map (key: {
        name = key;
        value =
          if selectedSource != "" && key == selectedSource then
            normalizeSource discardContext serviceName key (sources.${key} or { })
          else
            { };
      }) keys
    );

  sourceValue =
    serviceName: cfg: sources:
    let
      keys = sourceKeys cfg;
      selectedSource = resolveSelectedSource serviceName keys (cfg.defaultSource or "");
    in
    if selectedSource == "" then
      {
        inherit selectedSource;
        value = { };
      }
    else
      {
        inherit selectedSource;
        value = sources.${selectedSource} or { };
      };

  requireValue =
    {
      serviceName,
      mode,
      kind,
      field,
      value,
    }:
    if value == null || value == "" then
      throw "service '${serviceName}' check '${mode}' kind '${kind}' requires field '${field}'"
    else
      value;

  defaultWait = runtimeDefaults.probes.wait;

  normalizeWait =
    wait:
    defaultWait
    // dropNulls {
      enabled = if wait ? enabled then wait.enabled else null;
      timeoutSeconds = if wait ? timeoutSeconds then wait.timeoutSeconds else null;
      intervalSeconds = if wait ? intervalSeconds then wait.intervalSeconds else null;
      timeoutEnvVar = if wait ? timeoutEnvVar then wait.timeoutEnvVar else null;
      intervalEnvVar = if wait ? intervalEnvVar then wait.intervalEnvVar else null;
    };

  modePhaseLabel = mode: if mode == "health" then "health" else "readiness";
  modeSuccessLabel = mode: if mode == "health" then "healthy" else "ready";
  modeFailureLabel = mode: if mode == "health" then "unhealthy" else "not ready";

  stepLabelDefault =
    serviceName: step: if step.label or null != null then step.label else serviceName;

  normalizeCheckStep =
    serviceName: mode: step:
    let
      kind = requireValue {
        inherit serviceName mode;
        kind = "unknown";
        field = "kind";
        value = step.kind or null;
      };
      stepBase = {
        inherit kind;
        serviceLabel = stepLabelDefault serviceName step;
        phaseLabel = modePhaseLabel mode;
        successLabel = modeSuccessLabel mode;
        failureLabel = modeFailureLabel mode;
      };
    in
    if kind == "tcp" then
      stepBase
      // {
        endpoint = requireValue {
          inherit serviceName mode kind;
          field = "endpoint";
          value = step.endpoint or null;
        };
      }
    else if kind == "http" then
      stepBase
      // {
        endpoint = requireValue {
          inherit serviceName mode kind;
          field = "endpoint";
          value = step.endpoint or null;
        };
        path = step.path or "/";
      }
    else if kind == "jsonrpc" then
      stepBase
      // {
        endpoint = requireValue {
          inherit serviceName mode kind;
          field = "endpoint";
          value = step.endpoint or null;
        };
        method = requireValue {
          inherit serviceName mode kind;
          field = "method";
          value = step.method or "";
        };
      }
    else if kind == "exec" then
      stepBase
      // {
        command = requireValue {
          inherit serviceName mode kind;
          field = "command";
          value = step.command or "";
        };
      }
    else
      throw "service '${serviceName}' check '${mode}' uses unsupported kind '${kind}'";

  normalizeCheckPlan =
    serviceName: mode: plan:
    let
      normalizedSteps = map (normalizeCheckStep serviceName mode) (plan.steps or [ ]);
    in
    {
      count = builtins.length normalizedSteps;
      steps = normalizedSteps;
      wait = normalizeWait (plan.wait or { });
    };

  resolveCheckSummary =
    plan:
    let
      steps = plan.steps or [ ];
    in
    if steps == [ ] then
      "none"
    else if builtins.length steps == 1 then
      (builtins.elemAt steps 0).kind or "custom"
    else
      "composite";

  normalizeEndpoints =
    serviceName: endpoints:
    builtins.mapAttrs (endpointName: endpoint: {
      protocol = endpoint.protocol or "http";
      portKey = requireValue {
        serviceName = serviceName;
        mode = endpointName;
        kind = "endpoint";
        field = "portKey";
        value = endpoint.portKey or null;
      };
    }) endpoints;

  normalizeServiceConfig =
    {
      discardContext ? false,
      name,
      config,
    }:
    let
      cfgWithDefaults = config // {
        dataDirName = config.dataDirName or name;
        sourceKeys = sourceKeys config;
        sourceKinds = config.sourceKinds or { };
        requiredSourceArtifacts = config.requiredSourceArtifacts or [ ];
        checkRuntimeInputs = config.checkRuntimeInputs or [ ];
        endpoints = config.endpoints or { };
        checks = config.checks or { };
      };
      keys = sourceKeys cfgWithDefaults;
      selectedSourceName = resolveSelectedSource name keys (cfgWithDefaults.defaultSource or "");
      normalizedSources = normalizeSelectedSources discardContext name keys selectedSourceName (
        cfgWithDefaults.sources or { }
      );
      selected = sourceValue name cfgWithDefaults normalizedSources;
      package = selected.value.package or null;
      clientPackage = selected.value.clientPackage or null;
      normalizedEndpoints = normalizeEndpoints name (cfgWithDefaults.endpoints or { });
      probePlans = {
        health = normalizeCheckPlan name "health" (cfgWithDefaults.checks.health or { });
        ready = normalizeCheckPlan name "ready" (cfgWithDefaults.checks.ready or { });
      };
    in
    cfgWithDefaults
    // {
      sources = normalizedSources;
      sourceKeys = keys;
      package = package;
      clientPackage = clientPackage;
      resolved = {
        sources = normalizedSources;
        selectedSource = selected.selectedSource;
        endpoints = normalizedEndpoints;
        checks = {
          health = resolveCheckSummary probePlans.health;
          ready = resolveCheckSummary probePlans.ready;
        };
        probePlans = probePlans;
        operationProbes = probePlans;
        paths = {
          dataDirName = cfgWithDefaults.dataDirName;
        };
        runtime = dropNulls {
          inherit package clientPackage;
        };
        fixture = cfgWithDefaults.fixture or { };
      };
    };

  getProjectServiceEntry =
    {
      project,
      name,
    }:
    let
      services = project.services or { };
      serviceId = "service.${name}";
    in
    if builtins.hasAttr name services then
      services.${name}
    else if builtins.hasAttr serviceId services then
      services.${serviceId}
    else
      null;
in
{
  inherit
    getProjectServiceEntry
    normalizeServiceConfig
    ;

  getProjectServiceConfig =
    {
      project,
      name,
    }:
    let
      entry = getProjectServiceEntry {
        inherit
          project
          name
          ;
      };
      rawConfig =
        if entry == null then
          { }
        else if builtins.hasAttr "config" entry then
          entry.config
        else
          builtins.removeAttrs entry [
            "enable"
            "id"
            "name"
            "contract"
          ];
    in
    normalizeServiceConfig {
      inherit name;
      config = rawConfig;
    };

  isProjectServiceEnabled =
    {
      project,
      name,
    }:
    let
      entry = getProjectServiceEntry {
        inherit
          project
          name
          ;
      };
    in
    if entry == null then false else entry.enable or false;
}
