{ pkgs }:
let
  frameworkSource = builtins.readFile ../../nixfied/project/module.nix;
  executorSource = builtins.readFile ../../nixfied/runner/executor.nix;
in
assert pkgs.lib.hasInfix "printf 'INFO: %s\\n'" frameworkSource;
assert pkgs.lib.hasInfix "printf 'WARN: %s\\n'" frameworkSource;
assert pkgs.lib.hasInfix "printf 'ERROR: %s\\n'" frameworkSource;
assert pkgs.lib.hasInfix "printf 'OK: %s\\n'" frameworkSource;
assert pkgs.lib.hasInfix "printf 'SKIP: %s\\n'" frameworkSource;
assert pkgs.lib.hasInfix "echo \"ERROR: unknown task '$task_id'\"" executorSource;
assert pkgs.lib.hasInfix "echo \"ERROR: unknown workflow '$workflow_id'\"" executorSource;
pkgs.runCommand "log-prefix-contract" { } ''
  echo "OK: log prefix contracts are stable" > "$out"
''
