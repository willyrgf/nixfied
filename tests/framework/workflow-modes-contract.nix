{ pkgs }:
let
  runtimeMetadataSource = builtins.readFile ../../nixfied/framework/runtime/runtime-metadata.nix;
in
assert !(builtins.pathExists ../../nixfied/framework/runtime/workflow-modes.nix);
assert pkgs.lib.hasInfix "nixfied-kernel task load-runtime" runtimeMetadataSource;
assert pkgs.lib.hasInfix "nixfied-kernel task load-hook" runtimeMetadataSource;
assert pkgs.lib.hasInfix "nixfied-kernel workflow load-runtime" runtimeMetadataSource;
assert pkgs.lib.hasInfix "nixfied-kernel workflow resolve-mode" runtimeMetadataSource;
assert (!pkgs.lib.hasInfix "case \"$task_id\"" runtimeMetadataSource);
assert (!pkgs.lib.hasInfix "case \"$workflow_id\"" runtimeMetadataSource);
assert (!pkgs.lib.hasInfix "workflowPlanCases =" runtimeMetadataSource);
assert (!pkgs.lib.hasInfix "taskHookCases =" runtimeMetadataSource);
pkgs.runCommand "workflow-modes-contract" { } ''
  echo "OK: runtime metadata stays kernel-backed and generated workflow/task case tables stay deleted" > "$out"
''
