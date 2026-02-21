{ pkgs }:
let
  frameworkSource = builtins.readFile ../../nixfied/project/module.nix;
  executorSource = builtins.readFile ../../nixfied/runner/executor.nix;
  envSandboxSource = builtins.readFile ../../nixfied/runner/env-sandbox.nix;
in
assert pkgs.lib.hasInfix "printf 'INFO: %s\\n'" frameworkSource;
assert pkgs.lib.hasInfix "printf 'WARN: %s\\n'" frameworkSource;
assert pkgs.lib.hasInfix "printf 'ERROR: %s\\n'" frameworkSource;
assert pkgs.lib.hasInfix "printf 'OK: %s\\n'" frameworkSource;
assert pkgs.lib.hasInfix "printf 'SKIP: %s\\n'" frameworkSource;
assert pkgs.lib.hasInfix "echo \"ERROR: unknown task '$task_id'\"" executorSource;
assert pkgs.lib.hasInfix "echo \"ERROR: unknown workflow '$workflow_id'\"" executorSource;
assert pkgs.lib.hasInfix "echo \"INFO: hook $phase $hook_id start\"" executorSource;
assert pkgs.lib.hasInfix "echo \"OK: hook $phase $hook_id done\"" executorSource;
assert pkgs.lib.hasInfix "echo \"ERROR: hook $phase $hook_id failed exitCode=$hook_exit_code\""
  executorSource;
assert pkgs.lib.hasInfix "echo \"ERROR: sensitive passthrough env blocked name=$pass_name\""
  envSandboxSource;
pkgs.runCommand "log-prefix-contract" { } ''
  echo "OK: log prefix contracts are stable" > "$out"
''
