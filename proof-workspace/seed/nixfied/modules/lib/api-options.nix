{ lib }:
let
  t = lib.types;

  argSpec = {
    name = lib.mkOption {
      type = t.str;
      default = "";
    };
    kind = lib.mkOption {
      type = t.enum [
        "flag"
        "option"
        "positional"
      ];
      default = "option";
    };
    type = lib.mkOption {
      type = t.enum [
        "string"
        "int"
        "bool"
        "enum"
        "pathAbs"
        "pathRel"
        "json"
        "durationSec"
        "port"
      ];
      default = "string";
    };
    long = lib.mkOption {
      type = t.nullOr t.str;
      default = null;
    };
    short = lib.mkOption {
      type = t.nullOr t.str;
      default = null;
    };
    required = lib.mkOption {
      type = t.bool;
      default = false;
    };
    values = lib.mkOption {
      type = t.listOf t.str;
      default = [ ];
    };
    min = lib.mkOption {
      type = t.nullOr t.int;
      default = null;
    };
    max = lib.mkOption {
      type = t.nullOr t.int;
      default = null;
    };
    description = lib.mkOption {
      type = t.str;
      default = "";
    };
  };

  envSpec = {
    name = lib.mkOption { type = t.str; };
    type = lib.mkOption {
      type = t.enum [
        "string"
        "int"
        "bool"
        "enum"
        "pathAbs"
        "pathRel"
        "json"
        "durationSec"
        "port"
      ];
      default = "string";
    };
    required = lib.mkOption {
      type = t.bool;
      default = false;
    };
    values = lib.mkOption {
      type = t.listOf t.str;
      default = [ ];
    };
    default = lib.mkOption {
      type = t.nullOr (
        t.oneOf [
          t.str
          t.int
          t.bool
        ]
      );
      default = null;
    };
    aliases = lib.mkOption {
      type = t.listOf t.str;
      default = [ ];
    };
    sensitive = lib.mkOption {
      type = t.bool;
      default = false;
    };
    description = lib.mkOption {
      type = t.str;
      default = "";
    };
  };

  outputSpec = {
    mode = lib.mkOption {
      type = t.enum [
        "text"
        "kv"
        "json"
        "ndjson"
      ];
      default = "text";
    };
    channels = lib.mkOption {
      type = t.enum [
        "stdout"
        "logs"
        "both"
      ];
      default = "stdout";
    };
    keys = lib.mkOption {
      type = t.listOf t.str;
      default = [ ];
    };
  };

  behaviorSpec = {
    idempotent = lib.mkOption {
      type = t.bool;
      default = false;
    };

    effects = lib.mkOption {
      type = t.listOf (
        t.enum [
          "none"
          "writes-state"
          "starts-daemon"
          "network"
          "reads-secrets"
        ]
      );
      default = [ "none" ];
    };

    timeoutSec = lib.mkOption {
      type = t.int;
      default = 0;
    };
  };
in
{
  inherit
    argSpec
    envSpec
    outputSpec
    behaviorSpec
    ;

  commandApi = t.submodule {
    options = {
      version = lib.mkOption {
        type = t.int;
        default = 1;
      };

      commandClass = lib.mkOption {
        type = t.enum [
          "typed"
          "passthrough"
          "json"
          "batch-runner"
        ];
        default = "typed";
      };

      summary = lib.mkOption {
        type = t.str;
        default = "";
      };

      details = lib.mkOption {
        type = t.str;
        default = "";
      };

      usage = lib.mkOption {
        type = t.listOf t.str;
        default = [ ];
      };

      examples = lib.mkOption {
        type = t.listOf t.str;
        default = [ ];
      };

      category = lib.mkOption {
        type = t.str;
        default = "core";
      };

      args = lib.mkOption {
        type = t.listOf (t.submodule { options = argSpec; });
        default = [ ];
      };

      env = lib.mkOption {
        type = t.listOf (t.submodule { options = envSpec; });
        default = [ ];
      };

      outputs = lib.mkOption {
        type = t.nullOr (t.submodule { options = outputSpec; });
        default = null;
      };

      behavior = lib.mkOption {
        type = t.submodule { options = behaviorSpec; };
        default = { };
      };

      errors = lib.mkOption {
        type = t.submodule {
          options = {
            codes = lib.mkOption {
              type = t.attrsOf t.int;
              default = { };
            };
          };
        };
        default = { };
      };
    };
  };
}
