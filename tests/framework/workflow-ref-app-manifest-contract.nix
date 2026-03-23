{ pkgs }:
let
  frameworkLib = import ../../nixfied/framework/core {
    inherit pkgs;
    system = pkgs.system;
  };
  shellHelpers = import ./lib/shell-helpers.nix { inherit pkgs; };

  frameworkOutputs = frameworkLib.mkFlakeOutputs {
    projectRoot = ../..;
    projectModules = [ ../../nixfied/project/module.nix ];
    extraModules = [
      {
        nixfied.apps."ci-full-direct" = {
          id = "ci-full-direct";
          kind = "workflowRef";
          workflowId = "workflow.ci.full";
          summary = "Exact workflowRef manifest contract";
          description = "Ensures explicit workflowRef apps only materialize the targeted workflow.";
          usage = [ "nix run .#ci-full-direct" ];
          ownerFile = "tests/framework/workflow-ref-app-manifest-contract.nix";
        };
      }
    ];
    localOverrides = [ ];
  };
in
pkgs.runCommand "workflow-ref-app-manifest-contract" { } ''
  set -euo pipefail
  ${shellHelpers.shellPrelude}

  GREP=${pkgs.gnugrep}/bin/grep
  HEAD=${pkgs.coreutils}/bin/head
  JQ=${pkgs.jq}/bin/jq

  if "$GREP" -Fq 'run-selected-app.nix' ${frameworkOutputs.apps."ci-full-direct".program}; then
    fail "workflowRef app should use its direct runtime launcher"
  fi

  app_runtime="$("$GREP" -Eo '/nix/store/[^"[:space:]]+-nixfied-app-runtime[^"[:space:]]*/bin/nixfied-app-runtime[^"[:space:]]*' ${frameworkOutputs.apps."ci-full-direct".program} | "$HEAD" -n 1)"
  require_non_empty "$app_runtime" "app runtime"
  require_file "$app_runtime"

  orchestrator="$("$GREP" -Eo '/nix/store/[^"[:space:]]+-nixfied-orchestrator[^"[:space:]]*/bin/nixfied-orchestrator' "$app_runtime" | "$HEAD" -n 1)"
  require_non_empty "$orchestrator" "app orchestrator"
  require_file "$orchestrator"

  executor="$("$GREP" -Eo '/nix/store/[^"[:space:]]+-nixfied-executor[^"[:space:]]*/bin/nixfied-executor' "$orchestrator" | "$HEAD" -n 1)"
  require_non_empty "$executor" "app executor"
  require_file "$executor"

  manifest="$("$GREP" -Eo '/nix/store/[^"[:space:]]+-nixfied-execution-manifest\.json' "$executor" | "$HEAD" -n 1)"
  require_non_empty "$manifest" "execution manifest"
  require_file "$manifest"

  "$JQ" -e '.schema.kind == "nixfied-execution-manifest"' "$manifest" > /dev/null
  "$JQ" -e '(.workflows | keys) == ["workflow.ci.full"]' "$manifest" > /dev/null
  "$JQ" -e '(.workflows | has("workflow.ci.basic")) | not' "$manifest" > /dev/null
  "$JQ" -e '(.workflows | has("workflow.ci.app")) | not' "$manifest" > /dev/null
  "$JQ" -e '(.workflows | has("workflow.ci.env")) | not' "$manifest" > /dev/null

  echo "OK: explicit workflowRef apps embed exact execution manifests" > "$out"
''
