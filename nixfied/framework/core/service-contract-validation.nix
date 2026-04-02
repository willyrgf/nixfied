{ pkgs }:
let
  runtimePrimitives = import ./runtime-primitives.nix { };
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
  listUtils = import ./list-utils.nix;
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
  inherit (runtimePrimitives)
    runtimeLogLevels
    runtimeOutputModes
    runtimeLogLevelEnvName
    runtimeLogLevelAliases
    runtimeOutputModeEnvName
    runtimeOutputModeAliases
    ;
  supportedRuntimePrimitiveKeys = [
    "version"
    "logLevel"
    "outputMode"
  ];

  isAttrs = x: builtins.isAttrs x;
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
          - Define ${serviceName}.contract with version=1, operations, artifacts, and runtimePrimitives.
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

  validateServiceImplementationMatchesCatalogErrors =
    {
      serviceName,
      serviceApi,
      implementation,
    }:
    let
      ops = serviceApi.operations or { };
      implementationOps = implementation.operations or { };
      implementationOpErrors = builtins.concatLists (
        map (
          opName:
          let
            opCfg = ops.${opName};
            runtimeOp = opCfg.runtimeOp or opName;
          in
          if runtimeOp == null || runtimeOp == "" then
            [ ]
          else
            expect (builtins.hasAttr runtimeOp implementationOps) "${serviceName}.${opName}: runtime implementation is missing operation '${runtimeOp}'"
            ++
              expect (isScriptLike (implementationOps.${runtimeOp} or null))
                "${serviceName}.${opName}: runtime implementation operation '${runtimeOp}' must be string/path/derivation"
        ) (builtins.attrNames ops)
      );
    in
    if implementation == null then
      [ "${serviceName}: runtime implementation is required" ]
    else if !isAttrs implementation then
      [ "${serviceName}: runtime implementation must be an attribute set" ]
    else
      expect (
        (implementation.version or null) == 1
      ) "${serviceName}: runtime implementation version must be 1"
      ++ expect (
        implementation ? operations
      ) "${serviceName}: runtime implementation operations are required"
      ++ expect (isAttrs implementationOps) "${serviceName}: runtime implementation operations must be an attribute set"
      ++ implementationOpErrors;

  validateServiceImplementationsAgainstCatalog =
    {
      serviceApis,
      serviceImplementations,
    }:
    let
      names = sortedAttrNames serviceApis;
      errs = builtins.concatLists (
        map (
          serviceName:
          validateServiceImplementationMatchesCatalogErrors {
            inherit serviceName;
            serviceApi = serviceApis.${serviceName};
            implementation = serviceImplementations.${serviceName} or null;
          }
        ) names
      );
    in
    if errs == [ ] then
      serviceImplementations
    else
      throw ''
        Nixfied service runtime implementation violated:
        ${renderErrors errs}
      '';

  validateServiceImplementations =
    {
      serviceContracts,
      serviceImplementations,
    }:
    let
      validatedContracts = validateServiceContracts serviceContracts;
      names = sortedAttrNames validatedContracts;
      errs = builtins.concatLists (
        map (
          serviceName:
          validateServiceImplementationMatchesCatalogErrors {
            inherit serviceName;
            serviceApi = validatedContracts.${serviceName};
            implementation = serviceImplementations.${serviceName} or null;
          }
        ) names
      );
    in
    if errs == [ ] then
      serviceImplementations
    else
      throw ''
        Nixfied service runtime implementation violated:
        ${renderErrors errs}
      '';
in
{
  inherit
    sortedAttrNames
    validateServiceContract
    validateServiceContracts
    validateServiceImplementationsAgainstCatalog
    validateServiceImplementations
    ;
}
