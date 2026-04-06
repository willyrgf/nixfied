{ lib, ... }:
{
  nixfied = {
    services.helios.enable = lib.mkForce true;
    tasks = {
      "test.launcher.helios-required" = {
        id = "task.test.launcher.helios-required";
        summary = "Launcher Helios-gated task";
        description = "Present only when the compiled graph includes Helios.";
        requirements.services = [ "helios" ];
        runner.command = ''
          set -euo pipefail
          printf '%s\n' "OK: helios-gated task ran"
        '';
      };

      "test.launcher.control" = {
        id = "task.test.launcher.control";
        summary = "Launcher control task";
        description = "Survives launcher-based Helios exclusion.";
        runner.command = ''
          set -euo pipefail
          printf '%s\n' "OK: launcher control task ran"
        '';
      };
    };
  };
}
