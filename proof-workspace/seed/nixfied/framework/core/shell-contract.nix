# Typed shell app contract helpers and runtime validator.
{ pkgs }:

let
  inherit (pkgs) lib;
  kernelPackage = import ../runtime/kernel { inherit pkgs; };
  exitCodes = import ./exit-codes.nix;
  runtimePrimitives = import ./runtime-primitives.nix { };
  validation = import ./validation.nix { inherit pkgs; };
  inherit (validation)
    expect
    renderErrors
    isNonEmptyString
    isNonEmptyList
    isListOfNonEmptyStrings
    isEnvVarName
    ;

  supportedTypes = [
    "string"
    "int"
    "bool"
    "enum"
    "pathAbs"
    "pathRel"
    "port"
    "durationSec"
    "json"
  ];

  supportedArgKinds = [
    "flag"
    "option"
    "positional"
  ];

  supportedOutputModes = [
    "text"
    "kv"
    "json"
  ];

  supportedCommandClasses = [
    "typed"
    "passthrough"
    "json"
    "batch-runner"
  ];

  defaultFailureCodes = builtins.removeAttrs exitCodes [ "canceled" ];

  inherit (runtimePrimitives)
    runtimeLogLevels
    runtimeOutputModes
    runtimeLogLevelEnvName
    runtimeLogLevelAliases
    runtimeLogLevelDefault
    runtimeOutputModeEnvName
    runtimeOutputModeAliases
    runtimeOutputModeDefault
    mkRuntimePrimitiveEnvSpecs
    mkServiceRuntimePrimitivesV1
    ;

  hasPrefix =
    prefix: value:
    builtins.isString value
    && (builtins.stringLength value) >= (builtins.stringLength prefix)
    && (builtins.substring 0 (builtins.stringLength prefix) value) == prefix;

  isShortOpt = value: builtins.isString value && (builtins.match "^-.[^[:space:]]*$" value) != null;

  isLongOpt = value: builtins.isString value && (builtins.match "^--[^[:space:]]+$" value) != null;

  isSupportedType = value: builtins.elem value supportedTypes;
  isSupportedKind = value: builtins.elem value supportedArgKinds;
  isSupportedOutputMode = value: builtins.elem value supportedOutputModes;
  isSupportedCommandClass = value: builtins.elem value supportedCommandClasses;
  isPositiveExitCode = value: builtins.isInt value && value > 0 && value < 256;

  duplicatesOf =
    values:
    let
      uniq = lib.unique values;
    in
    builtins.filter (
      value: (builtins.length (builtins.filter (candidate: candidate == value) values)) > 1
    ) uniq;

  listUtils = import ./list-utils.nix;
  normalizeStringSet = listUtils.uniqueSorted;

  sameStringSet = expected: actual: normalizeStringSet expected == normalizeStringSet actual;

  findEnvSpecByName =
    {
      env,
      name,
    }:
    lib.findFirst (envSpec: (envSpec.name or "") == name) null env;

  validateRuntimePrimitiveEnvErrors =
    {
      name,
      env,
      envName,
      aliases,
      values,
    }:
    let
      envSpec = findEnvSpecByName {
        inherit env;
        name = envName;
      };
      prefix = "${name}: commandApi.env[${envName}]";
      actualAliases = if envSpec == null then [ ] else (envSpec.aliases or [ ]);
      actualValues = if envSpec == null then [ ] else (envSpec.values or [ ]);
      actualDefault = if envSpec == null || !(envSpec ? default) then null else envSpec.default;
      actualType = if envSpec == null then null else (envSpec.type or "string");
      actualRequired = if envSpec == null then false else (envSpec.required or false);
    in
    expect (envSpec != null) "${name}: commandApi.env must include ${envName} runtime primitive"
    ++ expect (envSpec == null || actualType == "enum") "${prefix}: type must be enum"
    ++ expect (
      envSpec == null || sameStringSet aliases actualAliases
    ) "${prefix}: aliases must be exactly [${builtins.concatStringsSep ", " aliases}]"
    ++ expect (
      envSpec == null || sameStringSet values actualValues
    ) "${prefix}: values must be exactly [${builtins.concatStringsSep ", " values}]"
    ++ expect (envSpec == null || !actualRequired) "${prefix}: required must be false"
    ++ expect (
      envSpec == null || actualDefault == null || builtins.elem actualDefault values
    ) "${prefix}: default must be one of [${builtins.concatStringsSep ", " values}] when set";

  validateArgSpecErrors =
    {
      appName,
      argSpec,
      index,
    }:
    let
      prefix = "${appName}.commandApi.args[${toString index}]";
      kind =
        if !(argSpec ? kind) || argSpec.kind == null then
          if (argSpec ? long) || (argSpec ? short) then "option" else "positional"
        else
          argSpec.kind;
      type = argSpec.type or (if kind == "flag" then "bool" else "string");
      values = argSpec.values or [ ];
      hasDefault = argSpec ? default;
      min = argSpec.min or null;
      max = argSpec.max or null;
    in
    if !builtins.isAttrs argSpec then
      [ "${prefix}: arg spec must be an attribute set" ]
    else
      expect (isNonEmptyString (argSpec.name or "")) "${prefix}: name must be a non-empty string"
      ++ expect (isSupportedKind kind) "${prefix}: kind must be one of ${builtins.concatStringsSep "|" supportedArgKinds}"
      ++ expect (isSupportedType type) "${prefix}: type must be one of ${builtins.concatStringsSep "|" supportedTypes}"
      ++ expect (
        kind == "positional" || isLongOpt (argSpec.long or "")
      ) "${prefix}: long must be set to --<token> for flag/option args"
      ++ expect (
        !(argSpec ? short) || isShortOpt (argSpec.short or "")
      ) "${prefix}: short must use -<char> format when set"
      ++ expect (
        kind != "positional" || (!(argSpec ? long) && !(argSpec ? short))
      ) "${prefix}: positional args cannot set long/short"
      ++ expect (kind != "flag" || type == "bool") "${prefix}: flag args must use type=bool"
      ++ expect (
        !hasDefault
        || (
          builtins.isString argSpec.default
          || builtins.isInt argSpec.default
          || builtins.isBool argSpec.default
        )
      ) "${prefix}: default must be string/int/bool when set"
      ++ expect (
        type != "enum" || (isNonEmptyList values && isListOfNonEmptyStrings values)
      ) "${prefix}: enum args must define values=[\"...\"]"
      ++ expect (type != "enum" || !(argSpec ? pattern)) "${prefix}: enum args cannot also define pattern"
      ++ expect (min == null || builtins.isInt min) "${prefix}: min must be an integer when set"
      ++ expect (max == null || builtins.isInt max) "${prefix}: max must be an integer when set"
      ++ expect ((min == null || max == null) || min <= max) "${prefix}: min must be <= max";

  validateEnvSpecErrors =
    {
      appName,
      envSpec,
      index,
    }:
    let
      prefix = "${appName}.commandApi.env[${toString index}]";
      type = envSpec.type or "string";
      values = envSpec.values or [ ];
      min = envSpec.min or null;
      max = envSpec.max or null;
      aliases = envSpec.aliases or [ ];
      hasDefault = envSpec ? default;
    in
    if !builtins.isAttrs envSpec then
      [ "${prefix}: env spec must be an attribute set" ]
    else
      expect (isEnvVarName (envSpec.name or "")) "${prefix}: name must be a valid env var token (A-Z0-9_)"
      ++ expect (isSupportedType type) "${prefix}: type must be one of ${builtins.concatStringsSep "|" supportedTypes}"
      ++ expect (
        type != "enum" || (isNonEmptyList values && isListOfNonEmptyStrings values)
      ) "${prefix}: enum env vars must define values=[\"...\"]"
      ++ expect (
        !(envSpec ? required) || builtins.isBool (envSpec.required or null)
      ) "${prefix}: required must be a boolean when set"
      ++ expect (
        !hasDefault
        || (
          builtins.isString envSpec.default
          || builtins.isInt envSpec.default
          || builtins.isBool envSpec.default
        )
      ) "${prefix}: default must be string/int/bool when set"
      ++ expect (
        !(envSpec ? sensitive) || builtins.isBool (envSpec.sensitive or null)
      ) "${prefix}: sensitive must be a boolean when set"
      ++ expect (
        builtins.isList aliases && builtins.all isEnvVarName aliases
      ) "${prefix}: aliases must be env var tokens"
      ++ expect (min == null || builtins.isInt min) "${prefix}: min must be an integer when set"
      ++ expect (max == null || builtins.isInt max) "${prefix}: max must be an integer when set"
      ++ expect ((min == null || max == null) || min <= max) "${prefix}: min must be <= max";

  validateFailureCodesErrors =
    {
      appName,
      failureCodes,
    }:
    let
      prefix = "${appName}.commandApi.failureCodes";
      names = if builtins.isAttrs failureCodes then builtins.attrNames failureCodes else [ ];
      badKeys = builtins.filter (key: !isNonEmptyString key) names;
      badValues = builtins.filter (key: !(isPositiveExitCode failureCodes.${key})) names;
    in
    expect (builtins.isAttrs failureCodes) "${prefix}: failureCodes must be an attribute set"
    ++ expect (badKeys == [ ]) "${prefix}: failure code names must be non-empty strings"
    ++
      expect (badValues == [ ])
        "${prefix}: failure code values must be integers in 1..255 (invalid: ${builtins.concatStringsSep ", " badValues})";

  validateAppContractErrors =
    {
      name,
      contract,
    }:
    let
      args = contract.args or [ ];
      env = contract.env or [ ];
      commandClass = contract.commandClass or null;
      allowUnknownArgs = contract.allowUnknownArgs or false;
      outputs = contract.outputs or { mode = "text"; };
      mode = outputs.mode or "text";
      argErrs = builtins.concatLists (
        lib.imap0 (
          index: argSpec:
          validateArgSpecErrors {
            appName = name;
            inherit argSpec index;
          }
        ) args
      );
      envErrs = builtins.concatLists (
        lib.imap0 (
          index: envSpec:
          validateEnvSpecErrors {
            appName = name;
            inherit envSpec index;
          }
        ) env
      );
      argNames = map (argSpec: argSpec.name or "") args;
      longNames = builtins.filter (token: token != "") (map (argSpec: argSpec.long or "") args);
      shortNames = builtins.filter (token: token != "") (map (argSpec: argSpec.short or "") args);
      envNames = map (envSpec: envSpec.name or "") env;
      envAliases = builtins.concatLists (map (envSpec: envSpec.aliases or [ ]) env);
      duplicateArgNames = duplicatesOf argNames;
      duplicateLongNames = duplicatesOf longNames;
      duplicateShortNames = duplicatesOf shortNames;
      duplicateEnvNames = duplicatesOf envNames;
      duplicateEnvAliases = duplicatesOf envAliases;
      runtimePrimitiveErrs =
        validateRuntimePrimitiveEnvErrors {
          inherit
            name
            env
            ;
          envName = runtimeLogLevelEnvName;
          aliases = runtimeLogLevelAliases;
          values = runtimeLogLevels;
        }
        ++ validateRuntimePrimitiveEnvErrors {
          inherit
            name
            env
            ;
          envName = runtimeOutputModeEnvName;
          aliases = runtimeOutputModeAliases;
          values = runtimeOutputModes;
        };
    in
    if contract == null then
      [ "${name}: missing commandApi" ]
    else if !builtins.isAttrs contract then
      [ "${name}: commandApi must be an attribute set" ]
    else
      expect (contract ? version) "${name}: commandApi.version is required"
      ++ expect (builtins.isInt (
        contract.version or null
      )) "${name}: commandApi.version must be an integer"
      ++ expect ((contract.version or null) == 2) "${name}: commandApi.version must be 2"
      ++ expect (isNonEmptyString (
        contract.name or ""
      )) "${name}: commandApi.name must be a non-empty string"
      ++ expect (
        !(contract ? allowUnknownArgs) || builtins.isBool (contract.allowUnknownArgs or null)
      ) "${name}: commandApi.allowUnknownArgs must be a boolean when set"
      ++ expect (contract ? commandClass) "${name}: commandApi.commandClass is required"
      ++ expect (isNonEmptyString commandClass) "${name}: commandApi.commandClass must be a non-empty string"
      ++ expect (isSupportedCommandClass commandClass) "${name}: commandApi.commandClass must be one of ${builtins.concatStringsSep "|" supportedCommandClasses}"
      ++ expect (builtins.isList args) "${name}: commandApi.args must be a list"
      ++ expect (builtins.isList env) "${name}: commandApi.env must be a list"
      ++ expect (builtins.isAttrs outputs) "${name}: commandApi.outputs must be an attribute set"
      ++ expect (isSupportedOutputMode mode) "${name}: commandApi.outputs.mode must be one of ${builtins.concatStringsSep "|" supportedOutputModes}"
      ++ expect (
        !(outputs ? keys) || isListOfNonEmptyStrings (outputs.keys or [ ])
      ) "${name}: commandApi.outputs.keys must be a list of non-empty strings when set"
      ++ expect (
        !(contract ? idempotent) || builtins.isBool (contract.idempotent or null)
      ) "${name}: commandApi.idempotent must be a boolean when set"
      ++ expect (
        commandClass != "typed" || !allowUnknownArgs
      ) "${name}: commandApi.commandClass=typed requires allowUnknownArgs=false"
      ++ expect (
        commandClass != "typed" || mode != "json"
      ) "${name}: commandApi.commandClass=typed cannot use outputs.mode=json"
      ++ expect (
        commandClass != "passthrough" || allowUnknownArgs
      ) "${name}: commandApi.commandClass=passthrough requires allowUnknownArgs=true"
      ++ expect (
        commandClass != "json" || mode == "json"
      ) "${name}: commandApi.commandClass=json requires outputs.mode=json"
      ++ expect (
        commandClass != "json" || !allowUnknownArgs
      ) "${name}: commandApi.commandClass=json requires allowUnknownArgs=false"
      ++ expect (
        commandClass != "batch-runner" || !allowUnknownArgs
      ) "${name}: commandApi.commandClass=batch-runner requires allowUnknownArgs=false"
      ++ validateFailureCodesErrors {
        appName = name;
        failureCodes = contract.failureCodes or defaultFailureCodes;
      }
      ++
        expect (duplicateArgNames == [ ])
          "${name}: commandApi.args has duplicate names: ${builtins.concatStringsSep ", " duplicateArgNames}"
      ++
        expect (duplicateLongNames == [ ])
          "${name}: commandApi.args has duplicate long options: ${builtins.concatStringsSep ", " duplicateLongNames}"
      ++
        expect (duplicateShortNames == [ ])
          "${name}: commandApi.args has duplicate short options: ${builtins.concatStringsSep ", " duplicateShortNames}"
      ++ expect (
        duplicateEnvNames == [ ]
      ) "${name}: commandApi.env has duplicate names: ${builtins.concatStringsSep ", " duplicateEnvNames}"
      ++
        expect (duplicateEnvAliases == [ ])
          "${name}: commandApi.env has duplicate aliases: ${builtins.concatStringsSep ", " duplicateEnvAliases}"
      ++ argErrs
      ++ envErrs
      ++ runtimePrimitiveErrs;

  validateAppContract =
    {
      name,
      contract,
    }:
    let
      errs = validateAppContractErrors { inherit name contract; };
    in
    if errs == [ ] then
      contract
    else
      throw ''
        Nixfied shell app contract violated for "${name}":
        ${renderErrors errs}
      '';

  tokenToArgName =
    token:
    let
      base0 =
        if hasPrefix "--" token then
          builtins.substring 2 ((builtins.stringLength token) - 2) token
        else
          token;
      base1 =
        if hasPrefix "-" base0 then
          builtins.substring 1 ((builtins.stringLength base0) - 1) base0
        else
          base0;
      base2 =
        let
          equalMatch = builtins.match "^([^=]+)=.*$" base1;
        in
        if equalMatch == null then base1 else builtins.head equalMatch;
    in
    builtins.replaceStrings [ "-" "." ":" " " "/" ] [ "_" "_" "_" "_" "_" ] base2;

  mkDefaultArgFromDoc =
    arg:
    let
      token =
        if builtins.isAttrs arg then
          (arg.name or "")
        else if builtins.isString arg then
          arg
        else
          "";
      name = tokenToArgName token;
      hasLong = hasPrefix "--" token;
      hasShort = (!hasLong) && hasPrefix "-" token;
      kind =
        if hasLong || hasShort then
          if (builtins.match "^--[^=]+=.+$" token) != null then "option" else "flag"
        else
          "positional";
      type = if kind == "flag" then "bool" else "string";
    in
    {
      inherit
        name
        kind
        type
        ;
    }
    // lib.optionalAttrs hasLong { long = token; }
    // lib.optionalAttrs hasShort { short = token; };

  mkDefaultEnvFromDoc =
    envDoc:
    let
      name =
        if builtins.isAttrs envDoc then
          (envDoc.name or "")
        else if builtins.isString envDoc then
          envDoc
        else
          "";
    in
    {
      inherit name;
      type = "string";
      required = false;
    };

  mkDefaultAppContract =
    {
      name,
      args ? [ ],
      env ? [ ],
      allowUnknownArgs ? false,
      commandClass ? "typed",
      outputsMode ? "text",
      failureCodes ? defaultFailureCodes,
      idempotent ? true,
    }:
    let
      envSpecs0 = map mkDefaultEnvFromDoc env;
      upsertEnvSpec =
        acc: spec: (builtins.filter (envSpec: (envSpec.name or "") != (spec.name or "")) acc) ++ [ spec ];
      envSpecs = builtins.foldl' upsertEnvSpec envSpecs0 (mkRuntimePrimitiveEnvSpecs { });
    in
    {
      version = 2;
      inherit
        name
        allowUnknownArgs
        commandClass
        idempotent
        failureCodes
        ;
      args = map mkDefaultArgFromDoc args;
      env = envSpecs;
      outputs = {
        mode = outputsMode;
      };
    };

  valueStrings = import ./value-string.nix;
  valueToString = valueStrings.toContractString;

  normalizeArgSpec =
    argSpec:
    let
      kind =
        if !(argSpec ? kind) || argSpec.kind == null then
          if (argSpec ? long) || (argSpec ? short) then "option" else "positional"
        else
          argSpec.kind;
      type = argSpec.type or (if kind == "flag" then "bool" else "string");
    in
    {
      name = argSpec.name or "";
      inherit
        kind
        type
        ;
      long = argSpec.long or "";
      short = argSpec.short or "";
      required = argSpec.required or false;
      values = argSpec.values or [ ];
      min = argSpec.min or null;
      max = argSpec.max or null;
    };

  normalizeEnvSpec = envSpec: {
    name = envSpec.name or "";
    type = envSpec.type or "string";
    required = envSpec.required or false;
    hasDefault = envSpec ? default;
    default = envSpec.default or null;
    min = envSpec.min or null;
    max = envSpec.max or null;
    values = envSpec.values or [ ];
    aliases = envSpec.aliases or [ ];
  };

  mkContractRuntime =
    {
      name,
      contract,
      logLevelDefault ? runtimeLogLevelDefault,
      outputModeDefault ? runtimeOutputModeDefault,
    }:
    let
      validated = validateAppContract { inherit name contract; };
      normalizedArgs = map normalizeArgSpec (validated.args or [ ]);
      normalizedEnv = map (
        envSpec:
        let
          normalized = normalizeEnvSpec envSpec;
        in
        if normalized.name == runtimeLogLevelEnvName then
          normalized
          // {
            hasDefault = true;
            default = logLevelDefault;
          }
        else if normalized.name == runtimeOutputModeEnvName then
          normalized
          // {
            hasDefault = true;
            default = outputModeDefault;
          }
        else
          normalized
      ) (validated.env or [ ]);
      failureCodes = validated.failureCodes or defaultFailureCodes;
    in
    pkgs.writeText "${name}-command-api-runtime.json" (
      builtins.toJSON {
        kind = "nixfied-command-runtime-plan";
        version = 1;
        allowUnknownArgs = validated.allowUnknownArgs or false;
        args = map (argSpec: {
          inherit (argSpec)
            name
            kind
            type
            long
            short
            required
            values
            min
            max
            ;
        }) normalizedArgs;
        env = map (
          envSpec:
          {
            inherit (envSpec)
              name
              type
              required
              min
              max
              values
              aliases
              ;
          }
          // lib.optionalAttrs envSpec.hasDefault {
            default = valueToString envSpec.default;
          }
        ) normalizedEnv;
        inherit failureCodes;
      }
    );

  runtime = pkgs.writeShellScript "nixfied-shell-contract-runtime" ''
    NIXFIED_CONTRACT_KERNEL="${kernelPackage}/bin/nixfied-kernel"
    NIXFIED_CONTRACT_RESOLVED_VALUE=""
    NIXFIED_CONTRACT_PLAN_LOADED="0"
    NIXFIED_CONTRACT_PLAN_FILE=""

    _nixfied_contract_err() {
      if command -v log_error >/dev/null 2>&1; then
        log_error "$*"
      else
        printf '%s\n' "ERROR: $*" >&2
      fi
    }

    _nixfied_contract_load_runtime_plan() {
      local contract_file="''${1:-}"
      local runtime_file="''${NIXFIED_COMMAND_API_RUNTIME:-}"

      if [ "''${NIXFIED_CONTRACT_PLAN_LOADED:-0}" = "1" ] && [ "$runtime_file" = "''${NIXFIED_CONTRACT_PLAN_FILE:-}" ]; then
        return 0
      fi

      if [ -z "$runtime_file" ]; then
        _nixfied_contract_err "app contract runtime plan not configured"
        if [ -n "$contract_file" ]; then
          _nixfied_contract_err "set NIXFIED_COMMAND_API_RUNTIME alongside NIXFIED_COMMAND_API_FILE=$contract_file"
        fi
        return 1
      fi

      if [ ! -f "$runtime_file" ]; then
        _nixfied_contract_err "app contract runtime plan missing path=$runtime_file"
        return 1
      fi

      NIXFIED_CONTRACT_PLAN_FILE="$runtime_file"
      NIXFIED_CONTRACT_PLAN_LOADED="1"
      return 0
    }

    nixfied_contract_resolve_runtime_primitives() {
      local log_level_default="''${1:-info}"
      local output_mode_default="''${2:-stdout}"
      local log_level="''${LOG_LEVEL:-''${NIXFIED_LOG_LEVEL:-$log_level_default}}"
      local output_mode="''${OUTPUT_MODE:-''${NIXFIED_OUTPUT_MODE:-$output_mode_default}}"

      case "$log_level" in
        error|warn|info|debug|trace) ;;
        *)
          _nixfied_contract_err "invalid LOG_LEVEL value=$log_level allowed=error,warn,info,debug,trace"
          return 2
          ;;
      esac

      case "$output_mode" in
        stdout|logs|both) ;;
        *)
          _nixfied_contract_err "invalid OUTPUT_MODE value=$output_mode allowed=stdout,logs,both"
          return 2
          ;;
      esac

      export LOG_LEVEL="$log_level" NIXFIED_LOG_LEVEL="$log_level"
      export OUTPUT_MODE="$output_mode" NIXFIED_OUTPUT_MODE="$output_mode"
      return 0
    }

    _nixfied_contract_eval_exports() {
      local export_text="$1"

      if [ -z "$export_text" ]; then
        return 0
      fi

      eval "$export_text"
    }

    nixfied_contract_validate_env() {
      local contract_file="''${1:-}"
      local export_text=""

      _nixfied_contract_load_runtime_plan "$contract_file" || return 2
      if export_text="$("$NIXFIED_CONTRACT_KERNEL" validate-input "$NIXFIED_CONTRACT_PLAN_FILE" env)"; then
        _nixfied_contract_eval_exports "$export_text"
      else
        return 2
      fi
      return 0
    }

    nixfied_contract_validate_args() {
      local contract_file="''${1:-}"
      shift || true
      local export_text=""

      _nixfied_contract_load_runtime_plan "$contract_file" || return 2
      if export_text="$("$NIXFIED_CONTRACT_KERNEL" validate-input "$NIXFIED_CONTRACT_PLAN_FILE" args -- "$@")"; then
        _nixfied_contract_eval_exports "$export_text"
      else
        return 2
      fi
      return 0
    }

    nixfied_contract_validate_exit() {
      local contract_file="''${1:-}"
      local exit_code="$2"

      _nixfied_contract_load_runtime_plan "$contract_file" || return 2
      "$NIXFIED_CONTRACT_KERNEL" validate-exit "$NIXFIED_CONTRACT_PLAN_FILE" "$exit_code" >/dev/null
    }
  '';
in
{
  inherit
    supportedTypes
    supportedArgKinds
    supportedOutputModes
    supportedCommandClasses
    runtimeLogLevels
    runtimeOutputModes
    runtimeLogLevelEnvName
    runtimeLogLevelAliases
    runtimeLogLevelDefault
    runtimeOutputModeEnvName
    runtimeOutputModeAliases
    runtimeOutputModeDefault
    mkRuntimePrimitiveEnvSpecs
    mkServiceRuntimePrimitivesV1
    defaultFailureCodes
    validateAppContractErrors
    validateAppContract
    mkDefaultAppContract
    mkContractRuntime
    runtime
    ;
}
