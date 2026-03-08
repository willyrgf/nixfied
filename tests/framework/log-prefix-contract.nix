{ pkgs }:
let
  frameworkSource = builtins.readFile ../../nixfied/project/module.nix;
  executorSource = builtins.readFile ../../nixfied/runner/executor.nix;
  envSandboxSource = builtins.readFile ../../nixfied/runner/env-sandbox.nix;
  helpersSource = builtins.readFile ../../nixfied/.framework/lib/helpers.nix;
  loggingRuntimeSource = builtins.readFile ../../nixfied/.framework/lib/logging-runtime.nix;
in
assert pkgs.lib.hasInfix "printf 'INFO: %s\\n'" frameworkSource;
assert pkgs.lib.hasInfix "printf 'WARN: %s\\n'" frameworkSource;
assert pkgs.lib.hasInfix "printf 'ERROR: %s\\n'" frameworkSource;
assert pkgs.lib.hasInfix "printf 'OK: %s\\n'" frameworkSource;
assert pkgs.lib.hasInfix "printf 'SKIP: %s\\n'" frameworkSource;
assert pkgs.lib.hasInfix "import ./logging-runtime.nix" helpersSource;
assert pkgs.lib.hasInfix "loggingPrelude = loggingRuntime.loggingPrelude;" helpersSource;
assert pkgs.lib.hasInfix "log_error()" loggingRuntimeSource;
assert pkgs.lib.hasInfix "_nixfied_emit 2 \"INFO: $*\" \"0\"" loggingRuntimeSource;
assert pkgs.lib.hasInfix "_nixfied_emit 2 \"OK: $*\" \"0\"" loggingRuntimeSource;
assert pkgs.lib.hasInfix "_nixfied_emit 2 \"SKIP: $*\" \"0\"" loggingRuntimeSource;
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
