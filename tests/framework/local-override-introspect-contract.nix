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
    localOverrides = [
      (
        { pkgs, ... }:
        {
          nixfied.packages.local-override-probe = pkgs.writeText "local-override-probe" "probe\n";
        }
      )
    ];
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
  "$JQ" -e '.payload.diagnostics.localOverridesActive == true' "$TMPDIR/check.json" > /dev/null
  "$JQ" -e '.payload.diagnostics.localOverrideCount == 1' "$TMPDIR/check.json" > /dev/null
  "$JQ" -e '.payload.diagnostics.legacyLocalDefault.active == false' "$TMPDIR/check.json" > /dev/null
  "$JQ" -e '.payload.diagnostics.legacyLocalDefault.status == "template-inactive"' "$TMPDIR/check.json" > /dev/null
  "$JQ" -e '.payload.diagnostics.legacyLocalDefault.message | contains("not loaded by nixfied")' "$TMPDIR/check.json" > /dev/null

  "$INTROSPECT_APP" check > "$TMPDIR/check-human.txt"
  require_contains "$TMPDIR/check-human.txt" "INFO: diagnostics.local_overrides_active=true"
  require_contains "$TMPDIR/check-human.txt" "INFO: diagnostics.legacy_local_default_active=false"
  require_contains "$TMPDIR/check-human.txt" "INFO: diagnostics.legacy_local_default_status=template-inactive"

  echo "OK: introspect reports local overrides without treating local/default.nix as active compatibility" > "$out"
''
