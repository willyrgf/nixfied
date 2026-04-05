# Fixture DSL compiler (Nix -> shell prelude)
{
  pkgs,
  project,
}:

let
  kernelExportRuntime = import ./kernel-export-runtime.nix { };
  lib = pkgs.lib;
  kernelPackage = import ../kernel { inherit pkgs; };
  serviceConfig = import ../../core/service-config.nix {
    inherit lib pkgs;
  };

  isEnvName = name: builtins.match "^[A-Za-z_][A-Za-z0-9_]*$" name != null;

  normalizeServiceSpec =
    spec:
    if builtins.isString spec then
      { name = spec; }
    else if builtins.isAttrs spec then
      spec
    else
      throw "Fixture service entry must be a string or attrset";

  getProjectServiceEntry =
    name:
    serviceConfig.getProjectServiceEntry {
      inherit project name;
    };

  getProjectServiceConfig =
    name:
    serviceConfig.getProjectServiceConfig {
      inherit project name;
    };

  getFixtureMetadata =
    serviceName:
    let
      cfg = getProjectServiceConfig serviceName;
    in
    cfg.resolved.fixture or { };

  assertService =
    serviceSpec:
    let
      name = serviceSpec.name or (throw "Fixture service entry missing `name`");
      entry = getProjectServiceEntry name;
      enabled = serviceConfig.isProjectServiceEnabled {
        inherit project name;
      };
    in
    if entry == null then
      throw "Unknown fixture service: ${name}"
    else if !enabled then
      throw "Fixture service `${name}` requires services.${name}.enable = true"
    else
      serviceSpec;

  quote = value: lib.escapeShellArg (toString value);

  coerceFixtureScalar =
    label: value:
    if builtins.isString value || builtins.isInt value || builtins.isBool value then
      toString value
    else
      throw "${label} must be a string, int, or bool";

  resolveInvocationArg =
    {
      label,
      argumentName,
      defaults,
      sourceAttrs,
    }:
    let
      rawValue =
        if builtins.hasAttr argumentName sourceAttrs then
          sourceAttrs.${argumentName}
        else if builtins.hasAttr argumentName defaults then
          defaults.${argumentName}
        else
          null;
    in
    if rawValue == null then
      throw "${label} is missing required argument `${argumentName}`"
    else
      coerceFixtureScalar "${label}.${argumentName}" rawValue;

  renderInvocationArgAssignments =
    {
      label,
      argumentFields,
      defaults ? { },
      sourceAttrs ? { },
    }:
    builtins.concatStringsSep "\n" (
      map (
        argumentName:
        if !isEnvName argumentName then
          throw "${label} uses non-shell-safe argument name `${argumentName}`"
        else
          "${argumentName}=${
            quote (resolveInvocationArg {
              inherit
                label
                argumentName
                defaults
                sourceAttrs
                ;
            })
          }"
      ) argumentFields
    );

  renderInvocationArgs =
    {
      label,
      argumentFields,
      defaults ? { },
      sourceAttrs ? { },
    }:
    builtins.concatStringsSep " " (
      map (
        argumentName:
        quote (resolveInvocationArg {
          inherit
            label
            argumentName
            defaults
            sourceAttrs
            ;
        })
      ) argumentFields
    );

  parseFixtureRef =
    from:
    let
      matches = builtins.match "^([A-Za-z0-9_-]+)\\.([A-Za-z0-9_.-]+)$" from;
    in
    if matches == null then
      throw "Unsupported fixtures.env.from value: ${from}"
    else
      {
        serviceName = builtins.elemAt matches 0;
        refName = builtins.elemAt matches 1;
      };

  normalizeBootstrapAction =
    {
      serviceName,
      metadata,
      action,
    }:
    let
      availableKinds = builtins.sort builtins.lessThan (builtins.attrNames metadata);
      inferredKind =
        if metadata ? default then
          "default"
        else if builtins.length availableKinds == 1 then
          builtins.head availableKinds
        else
          throw "Fixture bootstrap action for `${serviceName}` must specify `kind`";
    in
    if builtins.isString action then
      {
        kind = inferredKind;
        name = action;
      }
    else if builtins.isAttrs action then
      action // { kind = action.kind or inferredKind; }
    else
      throw "Fixture bootstrap action for `${serviceName}` must be string or attrset";

  mkExportTokenScript =
    {
      serviceName,
      serviceSpec,
      token,
    }:
    let
      metadata = (getFixtureMetadata serviceName).exports or { };
      tokenCfg =
        metadata.${token}
          or (throw "Unsupported fixture export token `${token}` for service `${serviceName}`");
      args = renderInvocationArgs {
        label = "fixture export ${serviceName}.${token}";
        argumentFields = tokenCfg.argumentFields or [ ];
        defaults = tokenCfg.defaults or { };
        sourceAttrs = serviceSpec;
      };
    in
    ''eval "$(svc ${quote serviceName} ${quote tokenCfg.operation}${
      lib.optionalString (args != "") " ${args}"
    })"'';

  mkBootstrapScript =
    serviceName: bootstrap:
    let
      metadata = (getFixtureMetadata serviceName).bootstrap or { };
      renderAction =
        action:
        let
          normalized = normalizeBootstrapAction {
            inherit
              serviceName
              metadata
              action
              ;
          };
          kind = normalized.kind or (throw "Fixture bootstrap action for `${serviceName}` missing `kind`");
          bootstrapCfg =
            metadata.${kind} or (throw "Unsupported fixture bootstrap kind for `${serviceName}`: ${kind}");
          args = renderInvocationArgs {
            label = "fixture bootstrap ${serviceName}.${kind}";
            argumentFields = bootstrapCfg.argumentFields or [ ];
            defaults = bootstrapCfg.defaults or { };
            sourceAttrs = normalized;
          };
        in
        "svc ${quote serviceName} ${quote bootstrapCfg.operation}${
          lib.optionalString (args != "") " ${args}"
        }";
    in
    lib.concatMapStringsSep "\n" renderAction bootstrap;

  mkServiceScript =
    {
      contextName,
      defaultProfile,
      defaultLogs,
      globalArtifacts,
    }:
    idx: rawSpec:
    let
      serviceSpec = assertService (normalizeServiceSpec rawSpec);
      serviceName = serviceSpec.name;
      profile = serviceSpec.profile or defaultProfile;
      timeout = toString (serviceSpec.timeout or 60);
      interval = toString (serviceSpec.interval or 1);
      exportsList = serviceSpec.exports or [ ];
      bootstrap = serviceSpec.bootstrap or [ ];
      logsEnabled =
        if serviceSpec ? logs then
          serviceSpec.logs
        else if globalArtifacts ? logs then
          globalArtifacts.logs
        else
          defaultLogs;
      logPrefix = if globalArtifacts ? prefix then toString globalArtifacts.prefix else contextName;
      logName =
        if serviceSpec ? logName then
          toString serviceSpec.logName
        else
          "${logPrefix}-${toString idx}-${serviceName}.log";

      exportScript = lib.concatMapStringsSep "\n" (
        token:
        mkExportTokenScript {
          inherit
            serviceName
            serviceSpec
            token
            ;
        }
      ) exportsList;

      bootstrapScript = mkBootstrapScript serviceName bootstrap;
    in
    ''
      {
        _fixture_log_file=""
        ${lib.optionalString logsEnabled ''
          _fixture_log_file="$(artifact_path ${quote logName})"
        ''}
        _fixture_keep_running="$(_fixture_keep_running_from_policy)"
        log_info "fixture service start name=${serviceName} profile=${profile}"
        fixture_start_service ${quote serviceName} ${quote profile} ${quote timeout} ${quote interval} "$_fixture_log_file" "$_fixture_keep_running"
        ${exportScript}
        ${bootstrapScript}
      }
    '';

  mkEnvExportScript =
    key: value:
    if !isEnvName key then
      throw "Invalid fixtures.env key (must be shell-safe env var name): ${key}"
    else if builtins.isString value || builtins.isInt value || builtins.isBool value then
      "export ${key}=${quote value}"
    else if builtins.isAttrs value then
      let
        from = value.from or (throw "fixtures.env.${key} requires `from` when value is an attrset");
      in
      if from == "env" then
        let
          varName = value.var or (throw "fixtures.env.${key} with from=\"env\" requires `var`");
        in
        ''
          export ${key}="''${${varName}:-}"
        ''
      else
        let
          ref = parseFixtureRef from;
          _ = assertService { name = ref.serviceName; };
          refMetadata = (getFixtureMetadata ref.serviceName).refs or { };
          refCfg = refMetadata.${ref.refName} or (throw "Unsupported fixture ref `${from}`");
          argAssignments = renderInvocationArgAssignments {
            label = "fixture ref ${from}";
            argumentFields = refCfg.argumentFields or [ ];
            defaults = refCfg.defaults or { };
            sourceAttrs = builtins.removeAttrs value [ "from" ];
          };
        in
        ''
          {
            ${argAssignments}
            export ${key}="$(
              ${refCfg.script}
            )"
          }
        ''
    else
      throw "fixtures.env.${key} must be a scalar or attrset";

  keepRunningFromPolicy = ''
    ${kernelExportRuntime.kernelExportRuntime}

    _fixture_keep_running_from_policy() {
      nixfied_load_kernel_exports "nixfied-fixture-policy" \
        ${kernelPackage}/bin/nixfied-kernel service-policy fixture-keep-running \
        "''${SERVICE_OWNER_SCOPE:-}" \
        "''${SERVICE_REUSE_POLICY:-}" \
        "''${SERVICE_DISCOVERY_SCOPE:-}" \
        || return 1
      echo "$KEEP_RUNNING"
    }
  '';

  renderPrelude =
    {
      fixtures ? null,
      contextName,
      defaultProfile ? "default",
      defaultLogs ? false,
    }:
    let
      cfg = if fixtures == null then { } else fixtures;
      services = cfg.services or [ ];
      envCfg = cfg.env or { };
      artifactsCfg = cfg.artifacts or { };
      keepRunningScript = if services == [ ] then "" else keepRunningFromPolicy;
      serviceScripts = lib.imap0 (mkServiceScript {
        inherit
          contextName
          defaultProfile
          defaultLogs
          ;
        globalArtifacts = artifactsCfg;
      }) services;
      envExports = lib.concatMapStringsSep "\n" (key: mkEnvExportScript key envCfg.${key}) (
        builtins.attrNames envCfg
      );
    in
    lib.concatStringsSep "\n" (
      [ ]
      ++ (if keepRunningScript == "" then [ ] else [ keepRunningScript ])
      ++ (if services == [ ] then [ ] else serviceScripts)
      ++ (if envExports == "" then [ ] else [ envExports ])
    );

  wrapScript =
    {
      script,
      fixtures ? null,
      contextName,
      defaultProfile ? "default",
      defaultLogs ? false,
    }:
    let
      prelude = renderPrelude {
        inherit
          fixtures
          contextName
          defaultProfile
          defaultLogs
          ;
      };
    in
    if prelude == "" then
      script
    else
      ''
        ${prelude}
        ${script}
      '';
in
{
  inherit
    renderPrelude
    wrapScript
    ;
}
