{ pkgs }:
let
  source = builtins.readFile ../../.github/workflows/nix.yml;
in
assert !(pkgs.lib.hasInfix "workflow-test" source);
assert pkgs.lib.hasInfix
  "nix config show | grep -E '^(cores|max-jobs|sandbox|substituters|system) = ' || true"
  source;
assert !(pkgs.lib.hasInfix "nix show-config" source);
assert pkgs.lib.hasInfix "isolation_log_source=\"$CI_DEBUG_DIR/isolation-cell.log\"" source;
assert pkgs.lib.hasInfix "isolation_logs_dir=\"$CI_DEBUG_DIR/isolation-logs\"" source;
assert pkgs.lib.hasInfix
  "sed -n 's/^INFO: test-isolation logs_root=//p' \"$isolation_log_source\" | tail -n 1"
  source;
assert pkgs.lib.hasInfix "cp -R \"$isolation_logs_root\"/. \"$isolation_logs_dir\"/" source;
assert pkgs.lib.hasInfix "find \"$isolation_logs_dir\" -type f -name summary.json | sort" source;
assert pkgs.lib.hasInfix
  "find \"$isolation_logs_dir\" -type f \\( -name validate.log -o -name run.log \\) | sort"
  source;
assert pkgs.lib.hasInfix
  "find \"$isolation_logs_dir\" -type f -path '*/registry/events.ndjson' | sort"
  source;
pkgs.runCommand "nix-ci-workflow-contract" { } ''
  echo "OK: nix CI workflow keeps shard usage in sync and captures isolation diagnostics" > "$out"
''
