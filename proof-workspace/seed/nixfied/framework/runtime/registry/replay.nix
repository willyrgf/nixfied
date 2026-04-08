{ pkgs }:
let
  events = import ./events.nix { inherit pkgs; };
  shellCommon = import ../../core/shell-common.nix { inherit pkgs; };
  kernelPackage = import ../kernel { inherit pkgs; };
in
{
  mkReplayTool =
    {
      name ? "nixfied-registry-replay",
    }:
    pkgs.writeShellScriptBin name ''
      set -euo pipefail

      ${events.mkShellLib { }}
      ${shellCommon}

      root="''${REGISTRY_ROOT:-}"
      if [ -z "$root" ] && [ "$#" -gt 0 ]; then
        root="$1"
      fi
      if [ -z "$root" ]; then
        nixfied_exit_usage "usage: ${name} <registry-root>"
      fi

      exec ${kernelPackage}/bin/nixfied-kernel registry replay "$root"
    '';
}
