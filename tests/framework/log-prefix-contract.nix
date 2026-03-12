{ pkgs }:
let
  projectSource = builtins.readFile ../../nixfied/project/module.nix;
  frameworkTestPresetSource = builtins.readFile ../../nixfied/framework/presets/framework-test.nix;
  nixChecksSource = builtins.readFile ../../nixfied/framework/core/mkNixChecks.nix;
  plainLoggingSource = builtins.readFile ../../nixfied/framework/core/plain-shell-logging.nix;
  executorSource = builtins.readFile ../../nixfied/framework/runtime/executor.nix;
  envSandboxSource = builtins.readFile ../../nixfied/framework/runtime/env-sandbox.nix;
  helpersSource = builtins.readFile ../../nixfied/framework/runtime/helpers/helpers.nix;
  loggingRuntimeSource = builtins.readFile ../../nixfied/framework/runtime/helpers/logging-runtime.nix;
in
assert pkgs.lib.hasInfix "frameworkTestPreset = import ../framework/presets/framework-test.nix"
  projectSource;
assert pkgs.lib.hasInfix "plainShellLogging = import ../core/plain-shell-logging.nix;"
  frameworkTestPresetSource;
assert pkgs.lib.hasInfix "plainShellLogging = import ./plain-shell-logging.nix;" nixChecksSource;
assert pkgs.lib.hasInfix "\"INFO: %s\\n\"" plainLoggingSource;
assert pkgs.lib.hasInfix "\"WARN: %s\\n\"" plainLoggingSource;
assert pkgs.lib.hasInfix "\"ERROR: %s\\n\"" plainLoggingSource;
assert pkgs.lib.hasInfix "\"OK: %s\\n\"" plainLoggingSource;
assert pkgs.lib.hasInfix "\"SKIP: %s\\n\"" plainLoggingSource;
assert pkgs.lib.hasInfix "import ./logging-runtime.nix" helpersSource;
assert pkgs.lib.hasInfix "loggingPrelude = loggingRuntime.loggingPrelude;" helpersSource;
assert pkgs.lib.hasInfix "log_error()" loggingRuntimeSource;
assert pkgs.lib.hasInfix "print_log_tail()" loggingRuntimeSource;
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
