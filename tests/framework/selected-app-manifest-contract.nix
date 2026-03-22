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

  orchestrator="$("$GREP" -Eo '/nix/store/[^"[:space:]]+-nixfied-orchestrator[^"[:space:]]*/bin/nixfied-orchestrator' "$app_wrapper" | "$HEAD" -n 1)"
  require_non_empty "$orchestrator" "app orchestrator"
  require_file "$orchestrator"

  executor="$("$GREP" -Eo '/nix/store/[^"[:space:]]+-nixfied-executor[^"[:space:]]*/bin/nixfied-executor' "$orchestrator" | "$HEAD" -n 1)"
  require_non_empty "$executor" "app executor"
  require_file "$executor"

  manifest="$("$GREP" -Eo '/nix/store/[^"[:space:]]+-nixfied-execution-manifest\.json' "$executor" | "$HEAD" -n 1)"
  require_non_empty "$manifest" "execution manifest"
  require_file "$manifest"

  "$JQ" -e '.schema.kind == "nixfied-execution-manifest"' "$manifest" > /dev/null
  "$JQ" -e '(.tasks | keys) == ["task.check"]' "$manifest" > /dev/null
  "$JQ" -e '(.workflows | keys) == []' "$manifest" > /dev/null
  "$JQ" -e '(.serviceCatalog | keys) == []' "$manifest" > /dev/null
  "$JQ" -e '.tasks."task.check".runner.package | tostring | contains("nix-checks")' "$manifest" > /dev/null
  "$JQ" -e '(.tasks | has("task.ci")) | not' "$manifest" > /dev/null

  echo "OK: selected app launchers embed narrowed execution manifests" > "$out"
''
