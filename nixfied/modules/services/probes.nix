{ lib }:
let
  t = lib.types;

  probeWaitSpec = t.submodule {
    options = {
      enabled = lib.mkOption {
        type = t.bool;
        default = false;
      };
      timeoutSeconds = lib.mkOption {
        type = t.ints.positive;
        default = 300;
      };
      intervalSeconds = lib.mkOption {
        type = t.ints.positive;
        default = 1;
      };
      timeoutEnvVar = lib.mkOption {
        type = t.nullOr t.str;
        default = null;
      };
      intervalEnvVar = lib.mkOption {
        type = t.nullOr t.str;
        default = null;
      };
    };
  };

  probeStepSpec = t.submodule {
    options = {
      kind = lib.mkOption {
        type = t.enum [
          "tcp"
          "http"
          "jsonrpc"
          "exec"
        ];
      };
      endpoint = lib.mkOption {
        type = t.nullOr t.str;
        default = null;
      };
      label = lib.mkOption {
        type = t.nullOr t.str;
        default = null;
      };
      host = lib.mkOption {
        type = t.str;
        default = "127.0.0.1";
      };
      path = lib.mkOption {
        type = t.str;
        default = "/";
      };
      method = lib.mkOption {
        type = t.str;
        default = "";
      };
      command = lib.mkOption {
        type = t.lines;
        default = "";
      };
    };
  };

  probePlanSpec = t.submodule {
    options = {
      steps = lib.mkOption {
        type = t.listOf probeStepSpec;
        default = [ ];
      };
      wait = lib.mkOption {
        type = t.nullOr probeWaitSpec;
        default = null;
      };
    };
  };
in
{
  probeWaitSpec = probeWaitSpec;
  probeStepSpec = probeStepSpec;
  probePlanSpec = probePlanSpec;
  probeOptions = {
    health = lib.mkOption {
      type = probePlanSpec;
      default = { };
    };
    ready = lib.mkOption {
      type = probePlanSpec;
      default = { };
    };
  };
}
