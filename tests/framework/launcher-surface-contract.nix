{
  pkgs,
  model,
  apps,
}:
let
  lib = pkgs.lib;
  wrappedAppNames = builtins.sort builtins.lessThan (
    lib.unique (
      (builtins.attrNames (model.views.apps or { }))
      ++ (builtins.filter (name: lib.hasPrefix "svc::" name) (builtins.attrNames apps))
      ++ (builtins.filter (name: builtins.hasAttr name apps) [
        "run-task"
        "run-workflow"
        "run-workflow-parallel"
      ])
    )
  );
  renderWrappedChecks = builtins.concatStringsSep "\n" (
    map (
      appName:
      let
        program = apps.${appName}.program;
      in
      ''
        require_selector_launcher ${lib.escapeShellArg appName} ${lib.escapeShellArg program}
      ''
    ) wrappedAppNames
  );
in
pkgs.runCommand "launcher-surface-contract" { } ''
  set -euo pipefail

  require_selector_launcher() {
    local app_name="$1"
    local program="$2"

    if ! ${pkgs.gnugrep}/bin/grep -Fq -- '--exclude-services' "$program"; then
      echo "missing --exclude-services selector in launcher for app=$app_name"
      exit 1
    fi

    if ! ${pkgs.gnugrep}/bin/grep -Fq -- '--launcher-help' "$program"; then
      echo "missing --launcher-help in launcher for app=$app_name"
      exit 1
    fi

    if ! ${pkgs.gnugrep}/bin/grep -Fq 'run-selected-app.nix' "$program"; then
      echo "missing second-stage app selection in launcher for app=$app_name"
      exit 1
    fi
  }

  ${renderWrappedChecks}

  if ${pkgs.gnugrep}/bin/grep -Fq 'run-selected-app.nix' ${apps.help.program}; then
    echo "help surface should not use the selector launcher"
    exit 1
  fi

  echo "OK: selector-aware launcher surfaces are wrapped" > "$out"
''
