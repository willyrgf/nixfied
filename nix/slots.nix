# Slot + environment utilities for the framework
{ pkgs, project }:

let
  cfg = project;
  projectMeta = cfg.project or { };

  envVar = projectMeta.envVar or "PROJECT_ENV";
  slotVar = projectMeta.slotVar or "NIX_ENV";

  slotsCfg = cfg.slots or { };
  slotMax = slotsCfg.max or 9;
  slotStride = slotsCfg.stride or 1;

  envOffsets = builtins.mapAttrs (name: envCfg: envCfg.offset or 0) (cfg.envs or { });
  envNames = builtins.attrNames envOffsets;
  defaultEnv =
    if builtins.hasAttr "dev" envOffsets then
      "dev"
    else if envNames != [ ] then
      builtins.head envNames
    else
      "dev";

  ports = cfg.ports or { };
  portNames = builtins.attrNames ports;

  baseDirExpr = (cfg.directories.base or "\${XDG_DATA_HOME:-$HOME/.local/share}/project");

  servicesCfg = cfg.services or { };
  rawServiceNames = servicesCfg.names or [ ];
  defaultServiceNames = if rawServiceNames != [ ] then rawServiceNames else portNames;
  defaultServiceSockets =
    if builtins.hasAttr "postgres" ports then
      { postgres = ".s.PGSQL.$PGPORT"; }
    else
      { };
  serviceSockets = defaultServiceSockets // (servicesCfg.sockets or { });
  serviceSocketNames = builtins.attrNames serviceSockets;
  uniqueNames =
    names:
    builtins.attrNames (builtins.listToAttrs (map (name: { inherit name; value = true; }) names));
  serviceNames = uniqueNames (defaultServiceNames ++ serviceSocketNames);

  normalizeName =
    name:
    let
      replaced = pkgs.lib.replaceStrings [ "-" "." ] [ "_" "_" ] name;
    in
    pkgs.lib.strings.toUpper replaced;

  portVarName = name: "${normalizeName name}_PORT";

  portAssignments = pkgs.lib.concatMapStringsSep "\n" (name: ''
    ${portVarName name}=$((${toString ports.${name}} + SLOT * ${toString slotStride} + ENV_OFFSET))
  '') portNames;

  portExports = pkgs.lib.concatMapStringsSep "\n" (
    name: "echo \"${portVarName name}=${"$"}${portVarName name}\""
  ) portNames;

  envCase = pkgs.lib.concatMapStringsSep "\n" (
    name: "  ${name}) ENV_OFFSET=${toString envOffsets.${name}} ;;"
  ) envNames;

  envList = pkgs.lib.concatStringsSep " " envNames;

  hasProd = builtins.hasAttr "prod" envOffsets;
  hasTest = builtins.hasAttr "test" envOffsets;
  hasDev = builtins.hasAttr "dev" envOffsets;

  # Resolve environment from PROJECT_ENV or command context
  resolveEnv = pkgs.writeShellScript "resolve-env" ''
    ENV_VAR="${envVar}"

    if [ -n "''${!ENV_VAR:-}" ]; then
      ENV_VALUE="''${!ENV_VAR}"
      case "$ENV_VALUE" in
        ${pkgs.lib.concatStringsSep "|" envNames})
          echo "$ENV_VALUE"
          exit 0
          ;;
        *)
          echo "Error: $ENV_VAR must be one of: ${envList} (got '$ENV_VALUE')" >&2
          exit 1
          ;;
      esac
    fi

    COMMAND="''${COMMAND_NAME:-''${0:-}}"
    ${pkgs.lib.optionalString hasProd ''
      if echo "$COMMAND" | grep -qi "prod"; then
        echo "prod"
        exit 0
      fi
    ''}
    ${pkgs.lib.optionalString hasTest ''
      if echo "$COMMAND" | grep -qiE "(test|ci)"; then
        echo "test"
        exit 0
      fi
    ''}
    ${pkgs.lib.optionalString hasDev ''
      if echo "$COMMAND" | grep -qi "dev"; then
        echo "dev"
        exit 0
      fi
    ''}

    echo "${defaultEnv}"
    exit 0
  '';

  # Validate and optionally prompt for slot/env
  requireSlotEnv = pkgs.writeShellScript "require-slot-env" ''
    SLOT_VAR="${slotVar}"
    ENV_VAR="${envVar}"

    SLOT="''${!SLOT_VAR:-0}"
    ENV="''${!ENV_VAR:-}"

    if [ -z "$ENV" ]; then
      ENV=$(${resolveEnv})
    fi

    if [ "$SLOT" -lt 0 ] || [ "$SLOT" -gt ${toString slotMax} ]; then
      echo "❌ Error: $SLOT_VAR must be 0-${toString slotMax} (got $SLOT)" >&2
      exit 1
    fi

    case "$ENV" in
      ${pkgs.lib.concatStringsSep "|" envNames})
        ;;
      *)
        echo "❌ Error: $ENV_VAR must be one of: ${envList} (got '$ENV')" >&2
        exit 1
        ;;
    esac

    case "$ENV" in
    ${envCase}
    esac

    ${portAssignments}

    NON_INTERACTIVE=false
    if [ -n "''${CI:-}" ] || [ -n "''${NO_TTY:-}" ] || [ "''${TERM:-}" = "dumb" ]; then
      NON_INTERACTIVE=true
    fi

    if [ "$NON_INTERACTIVE" = "false" ] && [ -t 0 ] && ([ -z "''${!SLOT_VAR:-}" ] || [ -z "''${!ENV_VAR:-}" ]); then
      echo "⚠️  $SLOT_VAR and/or $ENV_VAR not explicitly set" >&2
      echo "" >&2
      echo "Using defaults:" >&2
      echo "  $SLOT_VAR=$SLOT" >&2
      echo "  $ENV_VAR=$ENV" >&2
      echo "" >&2
      ${pkgs.lib.optionalString (portNames != [ ]) ''
        echo "Computed ports:" >&2
        ${pkgs.lib.concatMapStringsSep "\n" (name: ''
          echo "  ${normalizeName name}: ${"$"}${portVarName name}" >&2
        '') portNames}
        echo "" >&2
      ''}
      echo -n "Continue with these defaults? [y/N]: " >&2
      read -r REPLY
      if [[ ! "$REPLY" =~ ^[Yy]$ ]]; then
        echo "Aborted." >&2
        exit 1
      fi
    fi

    echo "SLOT=$SLOT"
    echo "ENV=$ENV"
  '';

  # Get slot + environment configuration as eval-able shell variables
  getSlotInfo = pkgs.writeShellScript "get-slot-info" ''
    SLOT_VAR="${slotVar}"
    ENV_VAR="${envVar}"

    SLOT="''${!SLOT_VAR:-0}"
    if [ "$SLOT" -lt 0 ] || [ "$SLOT" -gt ${toString slotMax} ]; then
      echo "Error: $SLOT_VAR must be 0-${toString slotMax} (got $SLOT)" >&2
      exit 1
    fi

    ENV=$(${resolveEnv})

    case "$ENV" in
    ${envCase}
    esac

    BASE_DIR="${baseDirExpr}"
    LOG_DIR="$BASE_DIR/logs-$SLOT-$ENV"
    RUN_DIR="$BASE_DIR/run-$SLOT-$ENV"
    CONFIG_DIR="$BASE_DIR/config-$SLOT-$ENV"
    STATE_DIR="$BASE_DIR/state-$SLOT-$ENV"

    ${portAssignments}

    ${pkgs.lib.concatMapStringsSep "\n" (name: let upper = normalizeName name; in ''
      ${upper}_DIR="${baseDirExpr}/${name}-$SLOT-$ENV"
      ${upper}_LOG_DIR="${baseDirExpr}/${name}-$SLOT-$ENV/logs"
      ${upper}_RUN_DIR="${baseDirExpr}/${name}-$SLOT-$ENV/run"
      ${upper}_CONFIG_DIR="${baseDirExpr}/${name}-$SLOT-$ENV/config"
      ${upper}_STATE_DIR="${baseDirExpr}/${name}-$SLOT-$ENV/state"
      ${upper}_SOCKET_DIR="${baseDirExpr}/${name}-$SLOT-$ENV/run/sockets"
    '') serviceNames}

    ${pkgs.lib.concatMapStringsSep "\n" (name: let upper = normalizeName name; in ''
      ${upper}_SOCKET_NAME="${serviceSockets.${name}}"
      if [ -n "${"$"}{${upper}_SOCKET_NAME:-}" ]; then
        ${upper}_SOCKET="${"$"}{${upper}_SOCKET_DIR}/${"$"}{${upper}_SOCKET_NAME}"
      fi
    '') serviceSocketNames}

    echo "SLOT=$SLOT"
    echo "ENV=$ENV"
    echo "ENV_OFFSET=$ENV_OFFSET"
    echo "SLOT_STRIDE=${toString slotStride}"
    echo "BASE_DIR=$BASE_DIR"
    echo "LOG_DIR=$LOG_DIR"
    echo "RUN_DIR=$RUN_DIR"
    echo "CONFIG_DIR=$CONFIG_DIR"
    echo "STATE_DIR=$STATE_DIR"
    ${pkgs.lib.concatMapStringsSep "\n" (name: let upper = normalizeName name; in ''
      echo "${upper}_DIR=${"$"}${upper}_DIR"
      echo "${upper}_LOG_DIR=${"$"}${upper}_LOG_DIR"
      echo "${upper}_RUN_DIR=${"$"}${upper}_RUN_DIR"
      echo "${upper}_CONFIG_DIR=${"$"}${upper}_CONFIG_DIR"
      echo "${upper}_STATE_DIR=${"$"}${upper}_STATE_DIR"
      echo "${upper}_SOCKET_DIR=${"$"}${upper}_SOCKET_DIR"
    '') serviceNames}
    ${pkgs.lib.concatMapStringsSep "\n" (name: let upper = normalizeName name; in ''
      if [ -n "${"$"}{${upper}_SOCKET:-}" ]; then
        echo "${upper}_SOCKET=${"$"}${upper}_SOCKET"
      fi
    '') serviceSocketNames}
    ${portExports}
  '';

  getServiceDir = service: "${baseDirExpr}/${service}-$SLOT-$ENV";

in
{
  inherit
    baseDirExpr
    envOffsets
    ports
    portVarName
    normalizeName
    resolveEnv
    requireSlotEnv
    getSlotInfo
    getServiceDir
    serviceNames
    serviceSockets
    slotMax
    slotStride
    ;
}
