{ lib, ... }:
let
  inherit (lib) mkOption types;
  positiveInt = types.addCheck types.int (value: value > 0);
  port = types.addCheck types.int (value: value >= 1 && value <= 65535);
in
{
  options.nixfied.services.synthetic = {
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

  options.nixfied.placement.ports = {
    base = mkOption {
      type = port;
      default = 38080;
      description = "Base TCP port for the first slot's candidate window.";
    };

    windowSize = mkOption {
      type = positiveInt;
      default = 11;
      description = "Number of candidate ports assigned to each slot.";
    };

    slotStride = mkOption {
      type = positiveInt;
      default = 100;
      description = "Port offset between adjacent slot candidate windows.";
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
