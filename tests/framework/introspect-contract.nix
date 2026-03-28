{
  pkgs,
  apps,
}:
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
  "$JQ" -e '.kind == "introspection-response" and .version == 1' "$TMPDIR/check.json" > /dev/null
  "$JQ" -e '.payload.resolved.nodeId == "app:check"' "$TMPDIR/check.json" > /dev/null
  "$JQ" -e '.payload.execution.launcherClass == "selected-app"' "$TMPDIR/check.json" > /dev/null
  "$JQ" -e '.payload.diagnostics.policyKind == "workspace-scoped"' "$TMPDIR/check.json" > /dev/null
  "$JQ" -e '.payload.diagnostics.legacyLocalDefault.path == "nixfied/local/default.nix"' "$TMPDIR/check.json" > /dev/null
  "$JQ" -e '.payload.diagnostics.legacyLocalDefault.present == true' "$TMPDIR/check.json" > /dev/null
  "$JQ" -e '.payload.diagnostics.legacyLocalDefault.customized == false' "$TMPDIR/check.json" > /dev/null
  "$JQ" -e '.payload.diagnostics.legacyLocalDefault.active == false' "$TMPDIR/check.json" > /dev/null
  "$JQ" -e '.payload.diagnostics.legacyLocalDefault.status == "template-inactive"' "$TMPDIR/check.json" > /dev/null

  "$INTROSPECT" check --why package:nix-checks --json > "$TMPDIR/why.json"
  "$JQ" -e '.kind == "introspection-response" and .version == 1' "$TMPDIR/why.json" > /dev/null
  "$JQ" -e '.payload.closure.target.nodeId == "package:nix-checks"' "$TMPDIR/why.json" > /dev/null
  "$JQ" -e '.payload.closure.reasonChains | length > 0' "$TMPDIR/why.json" > /dev/null
  "$JQ" -e '[.payload.closure.reasonChains[].nodes[].nodeId] | index("execution:app-manifest:check") != null' "$TMPDIR/why.json" > /dev/null
  "$JQ" -e '[.payload.closure.reasonChains[].nodes[].nodeId] | index("execution:full-model-manifest") == null' "$TMPDIR/why.json" > /dev/null

  "$INTROSPECT" reverse package:nix-checks --json > "$TMPDIR/reverse.json"
  "$JQ" -e '.kind == "introspection-response" and .version == 1' "$TMPDIR/reverse.json" > /dev/null
  "$JQ" -e '.payload.reverse.target.nodeId == "package:nix-checks"' "$TMPDIR/reverse.json" > /dev/null
  "$JQ" -e '.payload.reverse.reasonChains | length > 0' "$TMPDIR/reverse.json" > /dev/null

  "$INTROSPECT" service-set:default --json > "$TMPDIR/service-set.json"
  "$JQ" -e '.payload.resolved.nodeId == "service-set:default"' "$TMPDIR/service-set.json" > /dev/null
  "$JQ" -e '.payload.resolved.kind == "service-set"' "$TMPDIR/service-set.json" > /dev/null

  echo "OK: introspect surface resolves entities and explains package references" > "$out"
''
