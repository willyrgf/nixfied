# Nixfied service contract runtime projection helpers
{
  pkgs,
  shellContract ? import ./shell-contract.nix { inherit pkgs; },
}:

let
  runtimePrimitives = import ../../core/runtime-primitives.nix { };
  serviceContractValidation = import ../../core/service-contract-validation.nix {
    inherit pkgs;
  };
  inherit (serviceContractValidation)
    sortedAttrNames
    validateServiceContracts
    validateServiceAdapters
    ;
  inherit (runtimePrimitives)
    runtimeLogLevelDefault
    runtimeOutputModeDefault
    mkServiceRuntimePrimitivesV1
    ;

  serviceOps = contract: contract.operations or { };
  opPreRefs = op: op.preOps or [ ];
  opPostRefs = op: op.postOps or [ ];

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

  mkServiceHookEnv =
    ops:
    let
      pairs = map (op: {
        name = op.hookName;
        value = toString op.launcher;
      }) (builtins.filter (op: op.includeHook) ops);
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

  mkServiceHookEnvFromCatalog = args: mkServiceHookEnv (collectServiceOpsFromCatalog args);
  mkRuntimePrimitivesV1 = mkServiceRuntimePrimitivesV1;

in
{
  inherit
    collectServiceOpsFromCatalog
    mkServiceHookEnv
    mkRuntimePrimitivesV1
    mkServiceHookEnvFromCatalog
    ;
}
