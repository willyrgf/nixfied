{
  pkgs,
  apps,
}:
let
  shellHelpers = import ./lib/shell-helpers.nix { inherit pkgs; };
  projectRoot = builtins.toString ../..;
  frameworkRoot = builtins.toString ../../nixfied;
  launchNix = ../../nixfied/framework/launch/run-selected-app.nix;
  nixpkgsPath = builtins.toString pkgs.path;
in
pkgs.runCommand "selected-app-manifest-contract" { } ''
  set -euo pipefail
  ${shellHelpers.shellPrelude}

  NIX_BUILD=${pkgs.nix}/bin/nix-build
  GREP=${pkgs.gnugrep}/bin/grep
  HEAD=${pkgs.coreutils}/bin/head
  JQ=${pkgs.jq}/bin/jq

  if ! "$GREP" -Fq 'run-selected-app.nix' ${apps.check.program}; then
    fail "check app must resolve through run-selected-app.nix"
  fi

  selected_app="$(
    "$NIX_BUILD" ${launchNix} \
      --argstr system ${pkgs.system} \
      --argstr projectRoot ${projectRoot} \
      --argstr frameworkRoot ${frameworkRoot} \
      --argstr nixpkgsPath ${nixpkgsPath} \
      --argstr appName check \
      --argstr projectModuleSpecsJson "[]" \
      --argstr extraModuleSpecsJson "[]" \
      --argstr localOverrideSpecsJson "[]"
  )"
  require_non_empty "$selected_app" "selected app output"

  selected_launcher="$(${pkgs.findutils}/bin/find "$selected_app/bin" -maxdepth 1 -type f | "$HEAD" -n 1)"
  require_file "$selected_launcher"

  app_wrapper="$("$GREP" -Eo '/nix/store/[^"[:space:]]+-nixfied-[^"[:space:]]*/bin/nixfied-[^"[:space:]]*' "$selected_launcher" | "$HEAD" -n 1)"
  require_non_empty "$app_wrapper" "app wrapper"
  require_file "$app_wrapper"

  app_runtime="$("$GREP" -Eo '/nix/store/[^"[:space:]]+-nixfied-app-runtime[^"[:space:]]*/bin/nixfied-app-runtime[^"[:space:]]*' "$app_wrapper" | "$HEAD" -n 1)"
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
  "$JQ" -e '.compiled.runtimeMetadata.schema.kind == "nixfied-runtime-metadata"' "$manifest" > /dev/null
  "$JQ" -e '(.tasks | keys) == ["task.check"]' "$manifest" > /dev/null
  "$JQ" -e '(.workflows | keys) == []' "$manifest" > /dev/null
  "$JQ" -e '(.serviceCatalog | keys) == []' "$manifest" > /dev/null
  "$JQ" -e '.tasks."task.check".runner.package | tostring | contains("nix-checks")' "$manifest" > /dev/null
  "$JQ" -e '.compiled.runtimeMetadata.tasks."task.check".runner.type == "derivation"' "$manifest" > /dev/null
  "$JQ" -e '(.tasks | has("task.ci")) | not' "$manifest" > /dev/null

  selected_isolation_app="$(
    "$NIX_BUILD" ${launchNix} \
      --argstr system ${pkgs.system} \
      --argstr projectRoot ${projectRoot} \
      --argstr frameworkRoot ${frameworkRoot} \
      --argstr nixpkgsPath ${nixpkgsPath} \
      --argstr appName test-isolation \
      --argstr projectModuleSpecsJson "[]" \
      --argstr extraModuleSpecsJson "[]" \
      --argstr localOverrideSpecsJson "[]"
  )"
  require_non_empty "$selected_isolation_app" "selected isolation app output"

  selected_isolation_launcher="$(${pkgs.findutils}/bin/find "$selected_isolation_app/bin" -maxdepth 1 -type f | "$HEAD" -n 1)"
  require_file "$selected_isolation_launcher"

  isolation_wrapper="$("$GREP" -Eo '/nix/store/[^"[:space:]]+-nixfied-[^"[:space:]]*/bin/nixfied-[^"[:space:]]*' "$selected_isolation_launcher" | "$HEAD" -n 1)"
  require_non_empty "$isolation_wrapper" "test-isolation app wrapper"
  require_file "$isolation_wrapper"

  isolation_runtime="$("$GREP" -Eo '/nix/store/[^"[:space:]]+-nixfied-app-runtime[^"[:space:]]*/bin/nixfied-app-runtime[^"[:space:]]*' "$isolation_wrapper" | "$HEAD" -n 1)"
  require_non_empty "$isolation_runtime" "test-isolation app runtime"
  require_file "$isolation_runtime"

  isolation_orchestrator="$("$GREP" -Eo '/nix/store/[^"[:space:]]+-nixfied-orchestrator[^"[:space:]]*/bin/nixfied-orchestrator' "$isolation_runtime" | "$HEAD" -n 1)"
  require_non_empty "$isolation_orchestrator" "test-isolation orchestrator"
  require_file "$isolation_orchestrator"

  isolation_executor="$("$GREP" -Eo '/nix/store/[^"[:space:]]+-nixfied-executor[^"[:space:]]*/bin/nixfied-executor' "$isolation_orchestrator" | "$HEAD" -n 1)"
  require_non_empty "$isolation_executor" "test-isolation executor"
  require_file "$isolation_executor"

  isolation_manifest="$("$GREP" -Eo '/nix/store/[^"[:space:]]+-nixfied-execution-manifest\.json' "$isolation_executor" | "$HEAD" -n 1)"
  require_non_empty "$isolation_manifest" "test-isolation execution manifest"
  require_file "$isolation_manifest"

  "$JQ" -e '(.tasks | has("task.ops.test-isolation"))' "$isolation_manifest" > /dev/null
  "$JQ" -e '(.tasks | has("task.ops.validate-env"))' "$isolation_manifest" > /dev/null
  "$JQ" -e '(.tasks | has("task.test.isolation.probe"))' "$isolation_manifest" > /dev/null
  "$JQ" -e '(.tasks | has("task.test.isolation.unit"))' "$isolation_manifest" > /dev/null
  "$JQ" -e '(.workflows | has("workflow.test.isolation.probe"))' "$isolation_manifest" > /dev/null
  "$JQ" -e '.compiled.runtimeMetadata.workflows."workflow.test.isolation.probe".family == "test"' "$isolation_manifest" > /dev/null

  echo "OK: selected app launchers embed narrowed execution manifests" > "$out"
''
