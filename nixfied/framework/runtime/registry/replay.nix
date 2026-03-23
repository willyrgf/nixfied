{ pkgs }:
let
  events = import ./events.nix { inherit pkgs; };
  shellCommon = import ../../core/shell-common.nix { inherit pkgs; };
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

      events_file="$(registry_events_snapshot "$root")"
      if [ -z "$events_file" ] || [ ! -f "$events_file" ]; then
        echo "{}"
        exit 0
      fi

      ${pkgs.jq}/bin/jq -cs '
        reduce (
          sort_by((.payload.seq // 0))[]
          | select((.payload.taskId // "") != "" or (.payload.workflowId // "") != "")
        ) as $event ({};
          .[(if ($event.payload.taskId // "") != "" then "task:" + $event.payload.taskId else "workflow:" + ($event.payload.workflowId // "unknown") end)] = $event.payload.state
        )
      ' "$events_file"
      registry_snapshot_cleanup "$events_file"
    '';
}
