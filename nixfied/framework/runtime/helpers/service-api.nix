# Nixfied service contract helpers (validation + runtime projection)
{
  pkgs,
  appApi ? null,
  shellContract ? import ./shell-contract.nix { inherit pkgs; },
}:

let
  validation = import ./validation.nix { inherit pkgs; };
  inherit (validation)
    isNonEmptyString
    expect
    renderErrors
    sortedAttrNames
    optionalAttrSatisfies
    isListOfNonEmptyStrings
    isKVSpecList
    ;
  requiredLifecycleOps = [
    "start"
    "stop"
    "status"
  ];
  validCommandClasses = [
    "typed"
    "passthrough"
    "json"
    "batch-runner"
  ];
  runtimeLogLevels = shellContract.runtimeLogLevels;
  runtimeOutputModes = shellContract.runtimeOutputModes;
  runtimeLogLevelEnvName = shellContract.runtimeLogLevelEnvName;
  runtimeLogLevelAliases = shellContract.runtimeLogLevelAliases;
  runtimeLogLevelDefault = shellContract.runtimeLogLevelDefault;
  runtimeOutputModeEnvName = shellContract.runtimeOutputModeEnvName;
  runtimeOutputModeAliases = shellContract.runtimeOutputModeAliases;
  runtimeOutputModeDefault = shellContract.runtimeOutputModeDefault;
  supportedRuntimePrimitiveKeys = [
    "version"
    "logLevel"
    "outputMode"
  ];

  isAttrs = x: builtins.isAttrs x;
  tokenLib = import ./normalize-token.nix { lib = pkgs.lib; };
  normalizeToken = tokenLib.normalizeToken;
  listUtils = import ../../core/list-utils.nix;
  normalizeStringSet = listUtils.uniqueSorted;
  sameStringSet = expected: actual: normalizeStringSet expected == normalizeStringSet actual;
  isListOfOpNames = values: builtins.isList values && isListOfNonEmptyStrings values;
  isScriptLike = x: (builtins.isString x) || (builtins.isPath x) || (builtins.isAttrs x);

  opPreRefs = op: op.preOps or [ ];
  opPostRefs = op: op.postOps or [ ];
  opRefs = op: (opPreRefs op) ++ (opPostRefs op);
  opHasComposition = op: opRefs op != [ ];

  validateOpErrors =
    {
      serviceName,
      opName,
      op,
    }:
    let
      prefix = "${serviceName}.${opName}";
      runtimeOp = op.runtimeOp or opName;
    in
    if !isAttrs op then
      [ "${prefix}: operation must be an attribute set" ]
    else
      expect (
        runtimeOp != null || opHasComposition op
      ) "${prefix}: runtimeOp is required when preOps/postOps are not defined"
      ++ expect (
        !(op ? runtimeOp) || runtimeOp == null || isNonEmptyString runtimeOp
      ) "${prefix}: runtimeOp must be null or a non-empty string"
      ++ expect (op ? summary) "${prefix}: summary is required"
      ++ expect (isNonEmptyString (op.summary or "")) "${prefix}: summary must be a non-empty string"
      ++ expect (op ? details) "${prefix}: details is required"
      ++ expect (builtins.isString (op.details or null)) "${prefix}: details must be a string"
      ++ expect (optionalAttrSatisfies op "preOps"
        isListOfOpNames
      ) "${prefix}: preOps must be a list of non-empty strings"
      ++ expect (optionalAttrSatisfies op "postOps"
        isListOfOpNames
      ) "${prefix}: postOps must be a list of non-empty strings"
      ++ expect (optionalAttrSatisfies op "usage"
        isListOfNonEmptyStrings
      ) "${prefix}: usage must be a list of non-empty strings"
      ++ expect (optionalAttrSatisfies op "examples"
        isListOfNonEmptyStrings
      ) "${prefix}: examples must be a list of non-empty strings"
      ++ expect (optionalAttrSatisfies op "args"
        isKVSpecList
      ) "${prefix}: args must be a list of { name, description }"
      ++ expect (optionalAttrSatisfies op "env"
        isKVSpecList
      ) "${prefix}: env must be a list of { name, description }"
      ++ expect (
        !(op ? category) || op.category == null || isNonEmptyString op.category
      ) "${prefix}: category must be null or a non-empty string"
      ++ expect (optionalAttrSatisfies op "class"
        isNonEmptyString
      ) "${prefix}: class must be a non-empty string"
      ++ expect (
        !(op ? class) || builtins.elem op.class validCommandClasses
      ) "${prefix}: class must be one of ${builtins.concatStringsSep ", " validCommandClasses}"
      ++ expect (optionalAttrSatisfies op "idempotent"
        builtins.isBool
      ) "${prefix}: idempotent must be a boolean"
      ++ expect (optionalAttrSatisfies op "exposeApp"
        builtins.isBool
      ) "${prefix}: exposeApp must be a boolean"
      ++ expect (optionalAttrSatisfies op "exposeHook"
        builtins.isBool
      ) "${prefix}: exposeHook must be a boolean"
      ++ expect (
        !(op ? appName) || op.appName == null || isNonEmptyString op.appName
      ) "${prefix}: appName must be null or a non-empty string"
      ++ expect (
        !(op ? hook) || op.hook == null || isNonEmptyString op.hook
      ) "${prefix}: hook must be null or a non-empty string";

  validateOpCompositionErrors =
    {
      serviceName,
      ops,
    }:
    let
      names = builtins.attrNames ops;
      nameSet = builtins.listToAttrs (
        map (name: {
          inherit name;
          value = true;
        }) names
      );
      refErrorsFor =
        opName: field:
        let
          refs = ops.${opName}.${field} or [ ];
          unknown = builtins.filter (ref: !(builtins.hasAttr ref nameSet)) refs;
        in
        map (ref: "${serviceName}.${opName}: ${field} references unknown op '${ref}'") unknown;
      refErrs = builtins.concatLists (
        map (opName: (refErrorsFor opName "preOps") ++ (refErrorsFor opName "postOps")) names
      );
      cycleErrorsFor =
        path: current:
        let
          refs = builtins.filter (ref: builtins.hasAttr ref nameSet) (opRefs ops.${current});
        in
        builtins.concatLists (
          map (
            ref:
            if builtins.elem ref path then
              [
                "${serviceName}.${current}: op composition contains a cycle: ${
                  builtins.concatStringsSep " -> " (path ++ [ ref ])
                }"
              ]
            else
              cycleErrorsFor (path ++ [ ref ]) ref
          ) refs
        );
      cycleErrs = pkgs.lib.unique (
        builtins.concatLists (map (opName: cycleErrorsFor [ opName ] opName) names)
      );
    in
    refErrs ++ cycleErrs;

  validateRuntimePrimitiveSpecErrors =
    {
      serviceName,
      primitiveName,
      primitiveSpec,
      expectedEnv,
      expectedAliases,
      expectedValues,
    }:
    let
      prefix = "${serviceName}: contract.runtimePrimitives.${primitiveName}";
      envName = primitiveSpec.env or "";
      aliases = primitiveSpec.aliases or [ ];
      values = primitiveSpec.values or [ ];
      defaultValue = primitiveSpec.default or null;
    in
    if primitiveSpec == null then
      [ "${prefix} is required" ]
    else if !isAttrs primitiveSpec then
      [ "${prefix} must be an attribute set" ]
    else
      expect (primitiveSpec ? env) "${prefix}.env is required"
      ++ expect (isNonEmptyString envName) "${prefix}.env must be a non-empty string"
      ++ expect (envName == expectedEnv) "${prefix}.env must be ${expectedEnv}"
      ++ expect (primitiveSpec ? aliases) "${prefix}.aliases is required"
      ++ expect (
        builtins.isList aliases && isListOfNonEmptyStrings aliases
      ) "${prefix}.aliases must be a list of non-empty strings"
      ++ expect (sameStringSet expectedAliases aliases) "${prefix}.aliases must be [${builtins.concatStringsSep ", " expectedAliases}]"
      ++ expect (primitiveSpec ? values) "${prefix}.values is required"
      ++ expect (
        builtins.isList values && isListOfNonEmptyStrings values
      ) "${prefix}.values must be a list of non-empty strings"
      ++ expect (sameStringSet expectedValues values) "${prefix}.values must be [${builtins.concatStringsSep ", " expectedValues}]"
      ++ expect (primitiveSpec ? default) "${prefix}.default is required"
      ++ expect (isNonEmptyString (toString defaultValue)) "${prefix}.default must be a non-empty string"
      ++ expect (builtins.elem defaultValue expectedValues) "${prefix}.default must be one of [${builtins.concatStringsSep ", " expectedValues}]";

  validateRuntimePrimitivesErrors =
    {
      serviceName,
      runtimePrimitives,
    }:
    let
      keys =
        if runtimePrimitives == null || !isAttrs runtimePrimitives then
          [ ]
        else
          builtins.attrNames runtimePrimitives;
      unknownKeys = builtins.filter (k: !(builtins.elem k supportedRuntimePrimitiveKeys)) keys;
      logLevelSpec =
        if runtimePrimitives != null && isAttrs runtimePrimitives then
          runtimePrimitives.logLevel or null
        else
          null;
      outputModeSpec =
        if runtimePrimitives != null && isAttrs runtimePrimitives then
          runtimePrimitives.outputMode or null
        else
          null;
    in
    if runtimePrimitives == null then
      [ "${serviceName}: contract.runtimePrimitives is required" ]
    else if !isAttrs runtimePrimitives then
      [ "${serviceName}: contract.runtimePrimitives must be an attribute set" ]
    else
      expect (
        runtimePrimitives ? version
      ) "${serviceName}: contract.runtimePrimitives.version is required"
      ++ expect (builtins.isInt (
        runtimePrimitives.version or null
      )) "${serviceName}: contract.runtimePrimitives.version must be an integer"
      ++ expect (
        (runtimePrimitives.version or null) == 1
      ) "${serviceName}: contract.runtimePrimitives.version must be 1"
      ++
        expect (unknownKeys == [ ])
          "${serviceName}: contract.runtimePrimitives contains unsupported keys: ${builtins.concatStringsSep ", " unknownKeys}"
      ++ validateRuntimePrimitiveSpecErrors {
        inherit serviceName;
        primitiveName = "logLevel";
        primitiveSpec = logLevelSpec;
        expectedEnv = runtimeLogLevelEnvName;
        expectedAliases = runtimeLogLevelAliases;
        expectedValues = runtimeLogLevels;
      }
      ++ validateRuntimePrimitiveSpecErrors {
        inherit serviceName;
        primitiveName = "outputMode";
        primitiveSpec = outputModeSpec;
        expectedEnv = runtimeOutputModeEnvName;
        expectedAliases = runtimeOutputModeAliases;
        expectedValues = runtimeOutputModes;
      };

  validateServiceContractErrors =
    { serviceName, contract }:
    let
      version = contract.version or null;
      profiles = contract.profiles or [ ];
      ops = contract.operations or { };
      runtimePrimitives = contract.runtimePrimitives or null;
      missingLifecycleOps = builtins.filter (op: !(builtins.hasAttr op ops)) requiredLifecycleOps;
      opErrs = builtins.concatLists (
        map (
          name:
          validateOpErrors {
            inherit serviceName;
            opName = name;
            op = ops.${name};
          }
        ) (builtins.attrNames ops)
      );
      compositionErrs = validateOpCompositionErrors { inherit serviceName ops; };
      runtimeErrs = validateRuntimePrimitivesErrors { inherit serviceName runtimePrimitives; };
      adapter = contract.adapter or null;
    in
    if contract == null then
      [ "${serviceName}: missing contract" ]
    else if !isAttrs contract then
      [ "${serviceName}: contract must be an attribute set" ]
    else
      expect (contract ? version) "${serviceName}: contract.version is required"
      ++ expect (builtins.isInt (
        contract.version or null
      )) "${serviceName}: contract.version must be an integer"
      ++ expect (version == 1) "${serviceName}: contract.version must be 1"
      ++ expect (contract ? service) "${serviceName}: contract.service is required"
      ++ expect (isNonEmptyString (
        contract.service or ""
      )) "${serviceName}: contract.service must be a non-empty string"
      ++ expect (
        (contract.service or "") == serviceName
      ) "${serviceName}: contract.service must match service key (${serviceName})"
      ++ expect (contract ? summary) "${serviceName}: contract.summary is required"
      ++ expect (isNonEmptyString (
        contract.summary or ""
      )) "${serviceName}: contract.summary must be a non-empty string"
      ++ expect (contract ? details) "${serviceName}: contract.details is required"
      ++ expect (builtins.isString (
        contract.details or null
      )) "${serviceName}: contract.details must be a string"
      ++ expect (
        !(contract ? profiles) || isListOfNonEmptyStrings profiles
      ) "${serviceName}: contract.profiles must be a list of non-empty strings when set"
      ++ expect (contract ? ownerFile) "${serviceName}: contract.ownerFile is required"
      ++ expect (isNonEmptyString (
        contract.ownerFile or ""
      )) "${serviceName}: contract.ownerFile must be a non-empty string"
      ++ expect (contract ? adapter) "${serviceName}: contract.adapter is required"
      ++ expect (isAttrs adapter) "${serviceName}: contract.adapter must be an attribute set"
      ++ expect ((adapter.version or null) == 1) "${serviceName}: contract.adapter.version must be 1"
      ++ expect (adapter ? module) "${serviceName}: contract.adapter.module is required"
      ++ expect (
        !(adapter ? module) || builtins.pathExists adapter.module
      ) "${serviceName}: contract.adapter.module must point to an existing file"
      ++ expect (contract ? operations) "${serviceName}: contract.operations is required"
      ++ expect (isAttrs ops) "${serviceName}: contract.operations must be an attribute set"
      ++
        expect (missingLifecycleOps == [ ])
          "${serviceName}: contract.operations missing required lifecycle ops: ${builtins.concatStringsSep ", " missingLifecycleOps}"
      ++ expect (contract ? artifacts) "${serviceName}: contract.artifacts is required"
      ++ expect (isAttrs (
        contract.artifacts or null
      )) "${serviceName}: contract.artifacts must be an attribute set"
      ++ opErrs
      ++ compositionErrs
      ++ runtimeErrs;

  validateServiceContract =
    { serviceName, contract }:
    let
      errs = validateServiceContractErrors { inherit serviceName contract; };
    in
    if errs == [ ] then
      contract
    else
      throw ''
        Nixfied service contract violated for "${serviceName}":
        ${renderErrors errs}

        Fix:
          - Define ${serviceName}.contract with version=1, operations, artifacts, runtimePrimitives, and adapter.module.
      '';

  validateServiceContracts =
    serviceContracts:
    let
      names = sortedAttrNames serviceContracts;
      errs = builtins.concatLists (
        map (
          serviceName:
          validateServiceContractErrors {
            inherit serviceName;
            contract = serviceContracts.${serviceName};
          }
        ) names
      );
    in
    if errs == [ ] then
      serviceContracts
    else
      throw ''
        Nixfied service contract violated:
        ${renderErrors errs}
      '';

  validateServiceAdapterErrors =
    {
      serviceName,
      contract,
      adapter,
    }:
    let
      ops = contract.operations or { };
      adapterOps = adapter.operations or { };
      adapterOpErrors = builtins.concatLists (
        map (
          opName:
          let
            opCfg = ops.${opName};
            runtimeOp = opCfg.runtimeOp or opName;
          in
          if runtimeOp == null || runtimeOp == "" then
            [ ]
          else
            expect (builtins.hasAttr runtimeOp adapterOps) "${serviceName}.${opName}: runtime adapter is missing operation '${runtimeOp}'"
            ++
              expect (isScriptLike (adapterOps.${runtimeOp} or null))
                "${serviceName}.${opName}: runtime adapter operation '${runtimeOp}' must be string/path/derivation"
        ) (builtins.attrNames ops)
      );
    in
    if adapter == null then
      [ "${serviceName}: runtime adapter is required" ]
    else if !isAttrs adapter then
      [ "${serviceName}: runtime adapter must be an attribute set" ]
    else
      expect ((adapter.version or null) == 1) "${serviceName}: runtime adapter version must be 1"
      ++ expect (adapter ? operations) "${serviceName}: runtime adapter operations are required"
      ++ expect (isAttrs adapterOps) "${serviceName}: runtime adapter operations must be an attribute set"
      ++ adapterOpErrors;

  validateServiceAdapters =
    {
      serviceContracts,
      serviceAdapters,
    }:
    let
      validatedContracts = validateServiceContracts serviceContracts;
      names = sortedAttrNames validatedContracts;
      errs = builtins.concatLists (
        map (
          serviceName:
          validateServiceAdapterErrors {
            inherit serviceName;
            contract = validatedContracts.${serviceName};
            adapter = serviceAdapters.${serviceName} or null;
          }
        ) names
      );
    in
    if errs == [ ] then
      serviceAdapters
    else
      throw ''
        Nixfied service runtime adapter violated:
        ${renderErrors errs}
      '';

  serviceOps = contract: contract.operations or { };

  hookNameFor =
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

  sanitizeScriptToken = x: pkgs.lib.replaceStrings [ "/" ":" "." " " ] [ "-" "-" "-" "-" ] x;

  launcherNameFor =
    serviceName: opName: "service-op-${sanitizeScriptToken serviceName}-${sanitizeScriptToken opName}";

  runtimeScriptFor =
    {
      serviceName,
      opName,
      opCfg,
      adapterOps,
    }:
    let
      runtimeOp = opCfg.runtimeOp or opName;
    in
    if runtimeOp == null || runtimeOp == "" then null else adapterOps.${runtimeOp} or null;

  buildExecutionPlan =
    {
      serviceName,
      ops,
      adapterOps,
      opName,
      passArgs ? true,
    }:
    let
      opCfg = ops.${opName};
      currentScript = runtimeScriptFor {
        inherit
          serviceName
          opName
          opCfg
          adapterOps
          ;
      };
      mkNestedPlan =
        ref:
        buildExecutionPlan {
          inherit
            serviceName
            ops
            adapterOps
            ;
          opName = ref;
          passArgs = false;
        };
      currentStep =
        if currentScript == null then
          [ ]
        else
          [
            {
              inherit
                opName
                passArgs
                ;
              script = currentScript;
            }
          ];
    in
    builtins.concatLists (map mkNestedPlan (opPreRefs opCfg))
    ++ currentStep
    ++ builtins.concatLists (map mkNestedPlan (opPostRefs opCfg));

  mkServiceOpLauncher =
    {
      serviceName,
      opName,
      plan,
      runtimePrimitives,
    }:
    let
      logLevelDefault = runtimePrimitives.logLevel.default or runtimeLogLevelDefault;
      outputModeDefault = runtimePrimitives.outputMode.default or runtimeOutputModeDefault;
      renderPlanStep =
        step: if step.passArgs then ''${toString step.script} "$@"'' else "${toString step.script}";
    in
    pkgs.writeShellScript (launcherNameFor serviceName opName) ''
      set -euo pipefail

      source ${toString shellContract.runtime}
      nixfied_contract_resolve_runtime_primitives "${logLevelDefault}" "${outputModeDefault}"

      ${builtins.concatStringsSep "\n" (map renderPlanStep plan)}
    '';

  collectServiceOps =
    {
      serviceContracts,
      serviceAdapters,
    }:
    let
      names = sortedAttrNames serviceContracts;
      validatedContracts = validateServiceContracts serviceContracts;
      validatedAdapters = validateServiceAdapters {
        serviceContracts = validatedContracts;
        inherit serviceAdapters;
      };
      toOps =
        serviceName:
        let
          contract = validatedContracts.${serviceName};
          ops = serviceOps contract;
          adapterOps = validatedAdapters.${serviceName}.operations;
          opNamesSorted = sortedAttrNames ops;
        in
        map (
          opName:
          let
            opCfg = ops.${opName};
            appName =
              if (opCfg.appName or null) != null && opCfg.appName != "" then
                opCfg.appName
              else
                "svc::${serviceName}::${opName}";
            opRuntimePrimitives = contract.runtimePrimitives;
            plan = buildExecutionPlan {
              inherit
                serviceName
                ops
                adapterOps
                opName
                ;
            };
          in
          {
            inherit
              serviceName
              opName
              opCfg
              appName
              plan
              ;
            hookName = hookNameFor serviceName opName opCfg;
            includeApp = opCfg.exposeApp or true;
            usage = if opCfg ? usage then opCfg.usage else [ "nix run .#${appName}" ];
            category = if (opCfg.category or "") != "" then opCfg.category else serviceName;
            class = opCfg.class or "passthrough";
            idempotent = opCfg.idempotent or false;
            includeHook = opCfg.exposeHook or true;
            runtimePrimitives = opRuntimePrimitives;
            launcher = mkServiceOpLauncher {
              inherit
                serviceName
                opName
                plan
                ;
              runtimePrimitives = opRuntimePrimitives;
            };
          }
        ) opNamesSorted;
    in
    builtins.concatLists (map toOps names);

  collectServiceOpsFromCatalog =
    {
      serviceContracts,
      operationCatalog,
      serviceAdapters,
    }:
    let
      names = sortedAttrNames serviceContracts;
      validatedContracts = validateServiceContracts serviceContracts;
      validatedAdapters = validateServiceAdapters {
        serviceContracts = validatedContracts;
        inherit serviceAdapters;
      };
      toOps =
        serviceName:
        let
          contract = validatedContracts.${serviceName};
          ops = serviceOps contract;
          adapterOps = validatedAdapters.${serviceName}.operations;
          opCatalog = operationCatalog.${serviceName} or { };
          opNamesSorted = sortedAttrNames ops;
          missingCatalogOps = builtins.filter (opName: !(builtins.hasAttr opName opCatalog)) opNamesSorted;
        in
        if missingCatalogOps != [ ] then
          throw ''
            Nixfied service surface catalog is missing operation entries for "${serviceName}":
            ${builtins.concatStringsSep ", " missingCatalogOps}
          ''
        else
          map (
            opName:
            let
              opCfg = ops.${opName};
              opMetadata = opCatalog.${opName};
              opRuntimePrimitives = contract.runtimePrimitives;
              plan = buildExecutionPlan {
                inherit
                  serviceName
                  ops
                  adapterOps
                  opName
                  ;
              };
            in
            {
              inherit
                serviceName
                opName
                opCfg
                plan
                opMetadata
                ;
              appName = opMetadata.appName;
              hookName = opMetadata.hookName;
              includeApp = opMetadata.includeApp or false;
              usage = opMetadata.usage or [ "nix run .#${opMetadata.appName}" ];
              category = opMetadata.category or serviceName;
              class = opMetadata.class or "passthrough";
              idempotent = opMetadata.idempotent or false;
              includeHook = opMetadata.includeHook or false;
              runtimePrimitives = opRuntimePrimitives;
              launcher = mkServiceOpLauncher {
                inherit
                  serviceName
                  opName
                  plan
                  ;
                runtimePrimitives = opRuntimePrimitives;
              };
            }
          ) opNamesSorted;
    in
    builtins.concatLists (map toOps names);

  mkServiceHookEnvFromContracts =
    args:
    let
      ops = builtins.filter (op: op.includeHook) (collectServiceOps args);
      pairs = map (op: {
        name = op.hookName;
        value = toString op.launcher;
      }) ops;
      dedup =
        acc: pair:
        if builtins.hasAttr pair.name acc then
          throw "Nixfied service contract hook name collision: ${pair.name}"
        else
          acc
          // (builtins.listToAttrs [
            {
              name = pair.name;
              value = pair.value;
            }
          ]);
    in
    builtins.foldl' dedup { } pairs;

  mkServiceHookEnvFromCatalog =
    args:
    let
      ops = builtins.filter (op: op.includeHook) (collectServiceOpsFromCatalog args);
      pairs = map (op: {
        name = op.hookName;
        value = toString op.launcher;
      }) ops;
      dedup =
        acc: pair:
        if builtins.hasAttr pair.name acc then
          throw "Nixfied service contract hook name collision: ${pair.name}"
        else
          acc
          // (builtins.listToAttrs [
            {
              name = pair.name;
              value = pair.value;
            }
          ]);
    in
    builtins.foldl' dedup { } pairs;

  mkServiceAppProgramsFromContracts =
    args:
    let
      _ =
        if appApi == null then
          throw "mkServiceAppProgramsFromContracts requires appApi"
        else
          null;
      ops = builtins.filter (op: op.includeApp) (collectServiceOps args);
      pairs = map (op: {
        name = op.appName;
        value = (
          appApi.mkContractBackedApp {
            name = op.appName;
            script = ''
              exec ${toString op.launcher} "$@"
            '';
            contract = {
              class = op.class;
              summary = op.opCfg.summary;
              details = op.opCfg.details;
              usage = op.usage;
              examples = op.opCfg.examples or [ ];
              args = op.opCfg.args or [ ];
              env = op.opCfg.env or [ ];
              category = op.category;
              idempotent = op.idempotent;
            };
            env = { };
            useDeps = false;
            meta = {
              nixfied = {
                service = op.serviceName;
                operation = op.opName;
              };
            };
          }
        ).program;
      }) ops;
    in
    builtins.listToAttrs pairs;

  mkServiceAppProgramsFromCatalog =
    args:
    let
      _ =
        if appApi == null then
          throw "mkServiceAppProgramsFromCatalog requires appApi"
        else
          null;
      ops = builtins.filter (op: op.includeApp) (collectServiceOpsFromCatalog args);
      pairs = map (op: {
        name = op.appName;
        value = (
          appApi.mkContractBackedApp {
            name = op.appName;
            script = ''
              exec ${toString op.launcher} "$@"
            '';
            contract = {
              class = op.class;
              summary = op.opMetadata.summary;
              details = op.opMetadata.details;
              usage = op.usage;
              examples = op.opMetadata.examples or [ ];
              args = op.opMetadata.args or [ ];
              env = op.opMetadata.env or [ ];
              category = op.category;
              idempotent = op.idempotent;
            };
            env = { };
            useDeps = false;
            meta = {
              nixfied = {
                service = op.serviceName;
                operation = op.opName;
              };
            };
          }
        ).program;
      }) ops;
    in
    builtins.listToAttrs pairs;

  mkRuntimePrimitivesV1 =
    {
      logLevelDefault ? runtimeLogLevelDefault,
      outputModeDefault ? runtimeOutputModeDefault,
    }:
    shellContract.mkServiceRuntimePrimitivesV1 {
      inherit
        logLevelDefault
        outputModeDefault
        ;
    };
in
{
  inherit
    collectServiceOps
    collectServiceOpsFromCatalog
    validateServiceContract
    validateServiceContracts
    validateServiceAdapters
    mkRuntimePrimitivesV1
    mkServiceHookEnvFromContracts
    mkServiceHookEnvFromCatalog
    mkServiceAppProgramsFromContracts
    mkServiceAppProgramsFromCatalog
    ;
}
