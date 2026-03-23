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
    extraModules = [ ];
    localOverrides = [ ../../nixfied/local/default.nix ];
  };
in
pkgs.runCommand "local-override-introspect-contract" { } ''
  set -euo pipefail
  ${shellHelpers.shellPrelude}

  INTROSPECT_APP="${frameworkOutputs.apps.introspect.program}"
  JQ=${pkgs.jq}/bin/jq

  export NIXFIED_FLAKE_ROOT=${pkgs.lib.escapeShellArg repoRoot}
  cd "$NIXFIED_FLAKE_ROOT"

  "$INTROSPECT_APP" check --json > "$TMPDIR/check.json"
  "$JQ" -e '.diagnostics.localOverridesActive == true' "$TMPDIR/check.json" > /dev/null
  "$JQ" -e '.diagnostics.localOverrideCount == 1' "$TMPDIR/check.json" > /dev/null
  "$JQ" -e '.diagnostics.legacyLocalDefault.active == true' "$TMPDIR/check.json" > /dev/null
  "$JQ" -e '.diagnostics.legacyLocalDefault.status == "template-active"' "$TMPDIR/check.json" > /dev/null
  "$JQ" -e '.diagnostics.legacyLocalDefault.message | contains("explicitly loaded via localOverrides")' "$TMPDIR/check.json" > /dev/null

  "$INTROSPECT_APP" check > "$TMPDIR/check-human.txt"
  require_contains "$TMPDIR/check-human.txt" "INFO: diagnostics.local_overrides_active=true"
  require_contains "$TMPDIR/check-human.txt" "INFO: diagnostics.legacy_local_default_active=true"
  require_contains "$TMPDIR/check-human.txt" "INFO: diagnostics.legacy_local_default_status=template-active"

  echo "OK: introspect reports active legacy local/default.nix overrides truthfully" > "$out"
''
