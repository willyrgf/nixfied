{ pkgs }:
let
  frameworkLib = import ../../nixfied/framework/core {
    inherit pkgs;
    system = pkgs.system;
  };
  shellHelpers = import ./lib/shell-helpers.nix { inherit pkgs; };
  repoRoot = builtins.toString ../..;

  frameworkOutputs = frameworkLib.mkFlakeOutputs {
    projectRoot = ../..;
    projectModules = [ ../../nixfied/project/module.nix ];
    extraModules = [ ./service-set-enabled-module.nix ];
    localOverrides = [ ];
  };
in
pkgs.runCommand "service-set-surface-contract" { } ''
  set -euo pipefail
  ${shellHelpers.shellPrelude}

  EXPORT_APP="${frameworkOutputs.apps."services-export".program}"
  START_APP="${frameworkOutputs.apps."services-start".program}"
  INTROSPECT_APP="${frameworkOutputs.apps.introspect.program}"
  JQ=${pkgs.jq}/bin/jq
  export NIXFIED_FLAKE_ROOT=${pkgs.lib.escapeShellArg repoRoot}
  cd "$NIXFIED_FLAKE_ROOT"

  "$EXPORT_APP" -- --format json > "$TMPDIR/service-set.json"
  "$JQ" -e '.kind == "service-set-export" and .version == 1' "$TMPDIR/service-set.json" > /dev/null
  "$JQ" -e '.payload.serviceSetId == "service-set.default"' "$TMPDIR/service-set.json" > /dev/null
  "$JQ" -e '.payload.services | length == 2' "$TMPDIR/service-set.json" > /dev/null
  "$JQ" -e '.payload.services | map(.service) | sort == ["minio", "postgres"]' "$TMPDIR/service-set.json" > /dev/null
  "$JQ" -e '.payload.statePolicy.id == "workspace-scoped"' "$TMPDIR/service-set.json" > /dev/null
  "$JQ" -e '.payload.services | map(.resolvedArtifacts | has("dataDir")) | all' "$TMPDIR/service-set.json" > /dev/null

  "$EXPORT_APP" -- --format env > "$TMPDIR/service-set.env"
  require_contains "$TMPDIR/service-set.env" "NIXFIED_SERVICE_SET_ID=service-set.default"
  require_contains "$TMPDIR/service-set.env" "NIXFIED_SERVICE_SET_POSTGRES_SERVICE=postgres"
  require_contains "$TMPDIR/service-set.env" "NIXFIED_SERVICE_SET_MINIO_SERVICE=minio"

  "$START_APP" -- --help > "$TMPDIR/service-set-start-help.txt"
  require_contains "$TMPDIR/service-set-start-help.txt" "minio, postgres"

  "$INTROSPECT_APP" service-set:default --json > "$TMPDIR/service-set-introspect.json"
  "$JQ" -e '.payload.resolved.nodeId == "service-set:default"' "$TMPDIR/service-set-introspect.json" > /dev/null
  "$JQ" -e '.payload.resolution.data.requiredServices | sort == ["minio", "postgres"]' "$TMPDIR/service-set-introspect.json" > /dev/null

  echo "OK: service-set surfaces expose grouped export and introspection contracts" > "$out"
''
