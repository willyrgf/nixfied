# Nixfied service contract runtime projection helpers
{
  pkgs,
  appApi ? null,
  shellContract ? import ./shell-contract.nix { inherit pkgs; },
}:

let
  serviceContractValidation = import ../../core/service-contract-validation.nix {
    inherit
      pkgs
      shellContract
      ;
  };
  inherit (serviceContractValidation)
    sortedAttrNames
    validateServiceContract
    validateServiceContracts
    validateServiceAdapters
    ;
  runtimeLogLevels = shellContract.runtimeLogLevels;
  runtimeLogLevelDefault = shellContract.runtimeLogLevelDefault;
  runtimeOutputModeDefault = shellContract.runtimeOutputModeDefault;

  tokenLib = import ./normalize-token.nix { lib = pkgs.lib; };
  normalizeToken = tokenLib.normalizeToken;
  serviceOps = contract: contract.operations or { };
  opPreRefs = op: op.preOps or [ ];
  opPostRefs = op: op.postOps or [ ];

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
