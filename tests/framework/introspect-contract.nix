{
  pkgs,
  apps,
}:
let
  coreSource = builtins.readFile ../../nixfied/framework/core/mkCoreSurfaces.nix;
  runtimeSource = builtins.readFile ../../nixfied/framework/introspection/runtime.nix;
in
assert (!builtins.pathExists ../../nixfied/framework/core/introspection-query.py);
assert pkgs.lib.hasInfix "compile-introspection-bundle.nix" (builtins.readFile ../../nixfied/compiler/default.nix);
assert pkgs.lib.hasInfix "../introspection/runtime.nix" coreSource;
assert (!pkgs.lib.hasInfix "introspection-query.py" coreSource);
assert pkgs.lib.hasInfix ".resolutionIndex.explicit" runtimeSource;
pkgs.runCommand "introspect-contract" { } ''
  set -euo pipefail

  INTROSPECT=${apps.introspect.program}
  JQ=${pkgs.jq}/bin/jq
  GREP=${pkgs.gnugrep}/bin/grep

  "$INTROSPECT" check > "$TMPDIR/check-human.txt"
  "$GREP" -Fq 'INFO: resolved.node=app:check' "$TMPDIR/check-human.txt"
  "$GREP" -Fq 'INFO: execution.mapped_tasks=task.check' "$TMPDIR/check-human.txt"
  "$GREP" -Fq 'INFO: diagnostics.policy_kind=workspace-scoped' "$TMPDIR/check-human.txt"
  "$GREP" -Fq 'INFO: diagnostics.legacy_local_default_status=template-inactive' "$TMPDIR/check-human.txt"

  "$INTROSPECT" app:check --json > "$TMPDIR/check.json"
  "$JQ" -e '.resolved.nodeId == "app:check"' "$TMPDIR/check.json" > /dev/null
  "$JQ" -e '.execution.launcherClass == "selected-app"' "$TMPDIR/check.json" > /dev/null
  "$JQ" -e '.diagnostics.policyKind == "workspace-scoped"' "$TMPDIR/check.json" > /dev/null
  "$JQ" -e '.diagnostics.legacyLocalDefault.path == "nixfied/local/default.nix"' "$TMPDIR/check.json" > /dev/null
  "$JQ" -e '.diagnostics.legacyLocalDefault.present == true' "$TMPDIR/check.json" > /dev/null
  "$JQ" -e '.diagnostics.legacyLocalDefault.customized == false' "$TMPDIR/check.json" > /dev/null
  "$JQ" -e '.diagnostics.legacyLocalDefault.active == false' "$TMPDIR/check.json" > /dev/null
  "$JQ" -e '.diagnostics.legacyLocalDefault.status == "template-inactive"' "$TMPDIR/check.json" > /dev/null

  "$INTROSPECT" check --why package:nix-checks --json > "$TMPDIR/why.json"
  "$JQ" -e '.closure.target.nodeId == "package:nix-checks"' "$TMPDIR/why.json" > /dev/null
  "$JQ" -e '.closure.reasonChains | length > 0' "$TMPDIR/why.json" > /dev/null
  "$JQ" -e '[.closure.reasonChains[].nodes[].nodeId] | index("execution:app-manifest:check") != null' "$TMPDIR/why.json" > /dev/null
  "$JQ" -e '[.closure.reasonChains[].nodes[].nodeId] | index("execution:full-model-manifest") == null' "$TMPDIR/why.json" > /dev/null

  "$INTROSPECT" reverse package:nix-checks --json > "$TMPDIR/reverse.json"
  "$JQ" -e '.reverse.target.nodeId == "package:nix-checks"' "$TMPDIR/reverse.json" > /dev/null
  "$JQ" -e '.reverse.reasonChains | length > 0' "$TMPDIR/reverse.json" > /dev/null

  "$INTROSPECT" service-set:default --json > "$TMPDIR/service-set.json"
  "$JQ" -e '.resolved.nodeId == "service-set:default"' "$TMPDIR/service-set.json" > /dev/null
  "$JQ" -e '.resolved.kind == "service-set"' "$TMPDIR/service-set.json" > /dev/null

  echo "OK: introspect surface resolves entities and explains package references" > "$out"
''
