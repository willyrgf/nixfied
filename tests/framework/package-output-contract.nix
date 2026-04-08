{
  pkgs,
  serviceCatalog,
  packages,
  apps,
}:
let
  enabledServiceNames =
    builtins.map
      (
        serviceId:
        let
          service = serviceCatalog.${serviceId};
        in
        service.name or serviceId
      )
      (
        builtins.filter (serviceId: serviceCatalog.${serviceId}.enable or false) (
          builtins.attrNames serviceCatalog
        )
      );
  requiredPackageNames = [
    "nix-checks"
    "introspectionBundle"
    "introspectionGraph"
  ];
  requiredAppNames = [
    "docs"
    "features"
    "help"
    "introspect"
    "run-task"
    "run-workflow"
    "run-workflow-parallel"
    "runs"
    "stop-all-runs"
    "stop-run"
  ];
  missingPackages = builtins.filter (packageName: !(builtins.hasAttr packageName packages)) requiredPackageNames;
  missingApps = builtins.filter (appName: !(builtins.hasAttr appName apps)) requiredAppNames;
  hasServiceApps = builtins.any (appName: pkgs.lib.hasPrefix "svc::" appName) (builtins.attrNames apps);
in
assert missingPackages == [ ];
assert missingApps == [ ];
assert !(builtins.hasAttr "registry::replay" apps);
assert !(builtins.any (appName: pkgs.lib.hasPrefix "task::" appName) (builtins.attrNames apps));
assert if enabledServiceNames == [ ] then !hasServiceApps else hasServiceApps;
pkgs.runCommand "package-output-contract" { } ''
  set -euo pipefail

  require_exec() {
    local path="$1"
    if [ ! -x "$path" ]; then
      echo "missing executable: $path"
      exit 1
    fi
  }

  require_path() {
    local path="$1"
    if [ ! -e "$path" ]; then
      echo "missing path: $path"
      exit 1
    fi
  }

  require_exec "${packages."nix-checks"}/bin/nix-checks"
  require_path "${packages.introspectionGraph}"
  require_path "${packages.introspectionBundle}"
  require_exec "${apps.help.program}"
  require_exec "${apps.introspect.program}"
  require_exec "${apps.docs.program}"
  require_exec "${apps.features.program}"
  require_exec "${apps.run-task.program}"
  require_exec "${apps.run-workflow.program}"
  require_exec "${apps.run-workflow-parallel.program}"
  require_exec "${apps.runs.program}"
  require_exec "${apps.stop-run.program}"
  require_exec "${apps.stop-all-runs.program}"

  echo "OK: published packages and app entrypoints are present without help-text coupling" > "$out"
''
