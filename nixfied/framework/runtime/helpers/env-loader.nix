# Shared .env loading utilities.
{
  pkgs,
  project ? { },
  loggingPrelude ? "",
}:

let
  lib = pkgs.lib;
  kernelPackage = import ../kernel { inherit pkgs; };
  inherit (import ./validation.nix { inherit pkgs; })
    expect
    renderErrors
    isEnvVarName
    isScalar
    ;

  envFileCfg = ((project.tooling or { }).envFile or { });
  envFileEnabled = envFileCfg.enable or true;
  envFileStrict = envFileCfg.strict or false;
  allowSpecsRawValue = envFileCfg.allow or [ ];
  allowSpecsRaw = if builtins.isList allowSpecsRawValue then allowSpecsRawValue else [ ];

  supportedTypes = [
    "string"
    "int"
    "bool"
    "pathAbs"
    "pathRel"
    "port"
    "durationSec"
    "json"
  ];
  specErrors = builtins.concatLists (
    lib.imap0 (
      index: spec:
      let
        prefix = "tooling.envFile.allow[${toString index}]";
        type = spec.type or "string";
      in
      if !builtins.isAttrs spec then
        [ "${prefix}: entry must be an attribute set" ]
      else
        expect (isEnvVarName (spec.name or "")) "${prefix}.name must be a shell-safe env var token"
        ++ expect (
          !(spec ? type) || builtins.elem type supportedTypes
        ) "${prefix}.type must be one of ${builtins.concatStringsSep ", " supportedTypes}"
        ++ expect (
          !(spec ? required) || builtins.isBool spec.required
        ) "${prefix}.required must be a boolean when set"
        ++ expect (!(spec ? default) || isScalar spec.default) "${prefix}.default must be a scalar when set"
    ) allowSpecsRaw
  );
  specNames = map (spec: if builtins.isAttrs spec then (spec.name or "") else "") allowSpecsRaw;
  duplicateSpecNames = builtins.filter (
    name: (builtins.length (builtins.filter (candidate: candidate == name) specNames)) > 1
  ) (lib.unique specNames);
  topLevelErrs =
    expect (builtins.isList allowSpecsRawValue) "tooling.envFile.allow must be a list"
    ++
      expect (duplicateSpecNames == [ ])
        "tooling.envFile.allow contains duplicate names: ${builtins.concatStringsSep ", " duplicateSpecNames}";

  allErrs = topLevelErrs ++ specErrors;

  normalizedAllowSpecs = map (
    spec:
    if builtins.isAttrs spec then
      {
        name = spec.name or "";
        type = spec.type or "string";
        required = spec.required or false;
        hasDefault = spec ? default;
        default = spec.default or null;
        min = spec.min or null;
        max = spec.max or null;
      }
    else
      {
        name = "";
        type = "string";
        required = false;
        hasDefault = false;
        default = null;
        min = null;
        max = null;
      }
  ) allowSpecsRaw;

  validatedAllowSpecs =
    if allErrs == [ ] then
      normalizedAllowSpecs
    else
      throw ''
        Nixfied env-file config violated:
        ${renderErrors allErrs}
      '';

  valueToString =
    value:
    if value == null then
      ""
    else if builtins.isBool value then
      if value then "1" else "0"
    else
      toString value;

  allowSpecNames = map (spec: spec.name) (
    builtins.filter (spec: spec.name != "") validatedAllowSpecs
  );
  allowSpecFiles = builtins.listToAttrs (
    map (spec: {
      name = spec.name;
      value = pkgs.writeText "nixfied-env-spec-${spec.name}.json" (
        builtins.toJSON (
          (
            {
              inherit (spec)
                type
                required
                ;
              values = spec.values or [ ];
            }
            // lib.optionalAttrs (spec.min != null) {
              min = spec.min;
            }
            // lib.optionalAttrs (spec.max != null) {
              max = spec.max;
            }
          )
          // lib.optionalAttrs spec.hasDefault {
            default = valueToString spec.default;
          }
        )
      );
    }) (builtins.filter (spec: spec.name != "") validatedAllowSpecs)
  );

  allowSpecsRuntime = pkgs.writeText "nixfied-env-file-specs.sh" ''
      declare -ag NIXFIED_ENV_SPEC_NAMES=(
    ${lib.concatStringsSep "\n" (map (name: "  ${lib.escapeShellArg name}") allowSpecNames)}
      )
      declare -Ag NIXFIED_ENV_SPEC_TYPE=()
      declare -Ag NIXFIED_ENV_SPEC_REQUIRED=()
      declare -Ag NIXFIED_ENV_SPEC_HAS_DEFAULT=()
      declare -Ag NIXFIED_ENV_SPEC_DEFAULT=()
      declare -Ag NIXFIED_ENV_SPEC_FILE=()
    ${lib.concatStringsSep "\n" (
      map (
        spec:
        let
          name = spec.name;
        in
        ''
          NIXFIED_ENV_SPEC_TYPE[${lib.escapeShellArg name}]=${lib.escapeShellArg spec.type}
          NIXFIED_ENV_SPEC_REQUIRED[${lib.escapeShellArg name}]=${
            lib.escapeShellArg (if spec.required then "1" else "0")
          }
          NIXFIED_ENV_SPEC_HAS_DEFAULT[${lib.escapeShellArg name}]=${
            lib.escapeShellArg (if spec.hasDefault then "1" else "0")
          }
          NIXFIED_ENV_SPEC_DEFAULT[${lib.escapeShellArg name}]=${lib.escapeShellArg (valueToString spec.default)}
          NIXFIED_ENV_SPEC_FILE[${lib.escapeShellArg name}]=${
            lib.escapeShellArg (toString allowSpecFiles.${name})
          }
        ''
      ) (builtins.filter (spec: spec.name != "") validatedAllowSpecs)
    )}
  '';

  loadEnvFile = pkgs.writeShellScript "nixfied-load-env-file" ''
    ${loggingPrelude}

    set -euo pipefail

    ENV_FILE="''${1:-.env}"
    ENV_FILE_ENABLED="${if envFileEnabled then "1" else "0"}"
    ENV_FILE_STRICT="${if envFileStrict then "1" else "0"}"
    ENV_SPECS_RUNTIME="${allowSpecsRuntime}"
    if [ "''${BASH_SOURCE[0]:-}" != "$0" ]; then
      NIXFIED_ENV_LOADER_SOURCED=1
    else
      NIXFIED_ENV_LOADER_SOURCED=0
    fi
    source "$ENV_SPECS_RUNTIME"

    nixfied_env_spec_is_known() {
      local key="$1"
      [ -n "''${NIXFIED_ENV_SPEC_TYPE[$key]+x}" ]
    }

    validate_env_value() {
      local key="$1"
      local type="$2"
      local value="$3"
      local spec_file="''${NIXFIED_ENV_SPEC_FILE[$key]:-}"

      if [ -z "$spec_file" ] || [ ! -f "$spec_file" ]; then
        log_error "missing env spec file key=$key type=$type"
        return 1
      fi

      if ${kernelPackage}/bin/nixfied-kernel validate-scalar "$spec_file" "$value" >/dev/null 2>&1; then
        return 0
      fi

      log_error ".env key $key expects $type (got '$value')"
      return 1
    }

    nixfied_load_env_file_main() {
      if [ "$ENV_FILE_ENABLED" != "1" ]; then
        return 0
      fi

      if [ ! -f "$ENV_FILE" ]; then
        if [ "$ENV_FILE_STRICT" = "1" ]; then
          for SPEC_NAME in "''${NIXFIED_ENV_SPEC_NAMES[@]}"; do
            SPEC_REQUIRED="''${NIXFIED_ENV_SPEC_REQUIRED[$SPEC_NAME]}"
            if [ "$SPEC_REQUIRED" = "1" ] && [ -z "''${!SPEC_NAME:-}" ]; then
              log_error "required env key missing key=$SPEC_NAME source=.env"
              return 1
            fi
          done
        fi
        return 0
      fi

      while IFS= read -r RAW_LINE || [ -n "$RAW_LINE" ]; do
        LINE="$(printf '%s' "$RAW_LINE" | ${pkgs.gnused}/bin/sed -E 's/^[[:space:]]+//; s/[[:space:]]+$//')"
        case "$LINE" in
          \#*|"")
            continue
            ;;
        esac

        case "$LINE" in
          *=*)
            ;;
          *)
            log_error "invalid .env line (missing '=') line='$LINE'"
            return 1
            ;;
        esac

        key="$(printf '%s' "$LINE" | ${pkgs.gnused}/bin/sed -E 's/=.*$//; s/[[:space:]]+$//')"
        value="$(printf '%s' "$LINE" | ${pkgs.gnused}/bin/sed -E 's/^[^=]*=//')"

        if ! printf '%s' "$key" | ${pkgs.gnugrep}/bin/grep -Eq '^[A-Z_][A-Z0-9_]*$'; then
          log_error "invalid .env key token key='$key'"
          return 1
        fi

        value="$(printf '%s' "$value" | ${pkgs.gnused}/bin/sed -e 's/^"//' -e 's/"$//' -e "s/^'//" -e "s/'$//")"

        if nixfied_env_spec_is_known "$key"; then
          KNOWN_KEY="1"
        else
          KNOWN_KEY="0"
        fi

        if [ "$KNOWN_KEY" != "1" ] && [ "$ENV_FILE_STRICT" = "1" ]; then
          log_error "unknown .env key key=$key (strict mode enabled)"
          return 1
        fi

        if [ "$KNOWN_KEY" = "1" ]; then
          SPEC_TYPE="''${NIXFIED_ENV_SPEC_TYPE[$key]}"
          if [ -z "''${!key:-}" ]; then
            validate_env_value "$key" "$SPEC_TYPE" "$value" || return 1
          else
            validate_env_value "$key" "$SPEC_TYPE" "''${!key}" || return 1
          fi
        fi

        if [ -z "''${!key:-}" ]; then
          export "$key=$value"
        fi
      done < "$ENV_FILE"

      for SPEC_NAME in "''${NIXFIED_ENV_SPEC_NAMES[@]}"; do
        SPEC_TYPE="''${NIXFIED_ENV_SPEC_TYPE[$SPEC_NAME]}"
        SPEC_REQUIRED="''${NIXFIED_ENV_SPEC_REQUIRED[$SPEC_NAME]}"
        SPEC_HAS_DEFAULT="''${NIXFIED_ENV_SPEC_HAS_DEFAULT[$SPEC_NAME]}"

        if [ -z "''${!SPEC_NAME:-}" ]; then
          if [ "$SPEC_HAS_DEFAULT" = "1" ]; then
            SPEC_DEFAULT="''${NIXFIED_ENV_SPEC_DEFAULT[$SPEC_NAME]}"
            validate_env_value "$SPEC_NAME" "$SPEC_TYPE" "$SPEC_DEFAULT" || return 1
            export "$SPEC_NAME=$SPEC_DEFAULT"
          elif [ "$SPEC_REQUIRED" = "1" ]; then
            log_error "required env key missing key=$SPEC_NAME source=.env"
            return 1
          fi
        else
          validate_env_value "$SPEC_NAME" "$SPEC_TYPE" "''${!SPEC_NAME}" || return 1
        fi
      done

      return 0
    }

    NIXFIED_ENV_LOADER_RC=0
    nixfied_load_env_file_main || NIXFIED_ENV_LOADER_RC=$?
    if [ "$NIXFIED_ENV_LOADER_SOURCED" = "1" ]; then
      return "$NIXFIED_ENV_LOADER_RC"
    fi
    exit "$NIXFIED_ENV_LOADER_RC"
  '';

  loadEnv = pkgs.writeShellScript "nixfied-load-env" ''
    set -euo pipefail
    # Source into the current shell so exported keys persist for app scripts.
    source ${loadEnvFile} ".env"
  '';
in
{
  inherit
    loadEnvFile
    loadEnv
    ;
}
