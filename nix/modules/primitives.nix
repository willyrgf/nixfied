{ lib, ... }:
let
  inherit (lib) mkOption types;
  positiveInt = types.addCheck types.int (value: value > 0);
  port = types.addCheck types.int (value: value >= 1 && value <= 65535);
in
{
  options.nixfied.services.synthetic = {
    portWindow = {
      start = mkOption {
        type = port;
        default = 38080;
        description = "Start of the M0 synthetic service candidate port window.";
      };

      end = mkOption {
        type = port;
        default = 38090;
        description = "End of the M0 synthetic service candidate port window.";
      };
    };

    readiness = {
      timeoutMs = mkOption {
        type = positiveInt;
        default = 1000;
        description = "M0 readiness probe timeout in milliseconds.";
      };

      retryIntervalMs = mkOption {
        type = positiveInt;
        default = 100;
        description = "M0 readiness retry interval in milliseconds.";
      };

      maxAttempts = mkOption {
        type = positiveInt;
        default = 20;
        description = "M0 readiness max attempts.";
      };
    };

    stopTimeoutMs = mkOption {
      type = positiveInt;
      default = 5000;
      description = "M0 synthetic service stop timeout.";
    };
  };

  options.nixfied.tasks.smoke.timeoutMs = mkOption {
    type = positiveInt;
    default = 30000;
    description = "M0 smoke task timeout.";
  };

  options.nixfied.secrets = mkOption {
    type = types.listOf types.attrs;
    default = [ ];
    description = "M0 rejects non-empty secret descriptors.";
  };

  options.nixfied.workflows = mkOption {
    type = types.attrs;
    default = { };
    description = "M0 rejects workflow declarations.";
  };
}
