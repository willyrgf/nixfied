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

  "$INTROSPECT" app:check --json > "$TMPDIR/check.json"
  "$JQ" -e '.resolved.nodeId == "app:check"' "$TMPDIR/check.json" > /dev/null
  "$JQ" -e '.execution.launcherClass == "selected-app"' "$TMPDIR/check.json" > /dev/null
  "$JQ" -e '.diagnostics.policyKind == "workspace-scoped"' "$TMPDIR/check.json" > /dev/null

  "$INTROSPECT" check --why package:nix-checks --json > "$TMPDIR/why.json"
  "$JQ" -e '.closure.target.nodeId == "package:nix-checks"' "$TMPDIR/why.json" > /dev/null
  "$JQ" -e '.closure.reasonChains | length > 0' "$TMPDIR/why.json" > /dev/null
  "$JQ" -e '[.closure.reasonChains[].nodes[].nodeId] | index("execution:app-manifest:check") != null' "$TMPDIR/why.json" > /dev/null
  "$JQ" -e '[.closure.reasonChains[].nodes[].nodeId] | index("execution:full-model-manifest") == null' "$TMPDIR/why.json" > /dev/null

  "$INTROSPECT" reverse package:nix-checks --json > "$TMPDIR/reverse.json"
  "$JQ" -e '.reverse.target.nodeId == "package:nix-checks"' "$TMPDIR/reverse.json" > /dev/null
  "$JQ" -e '.reverse.reasonChains | length > 0' "$TMPDIR/reverse.json" > /dev/null

  echo "OK: introspect surface resolves entities and explains package references" > "$out"
''
