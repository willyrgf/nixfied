{ pkgs }:
let
  runtimeMetadataSource = builtins.readFile ../../nixfied/framework/runtime/runtime-metadata.nix;
in
assert !(builtins.pathExists ../../nixfied/framework/runtime/workflow-modes.nix);
assert pkgs.lib.hasInfix "nixfied-kernel task load-runtime" runtimeMetadataSource;
assert pkgs.lib.hasInfix "nixfied-kernel task load-hook" runtimeMetadataSource;
assert pkgs.lib.hasInfix "nixfied-kernel workflow load-runtime" runtimeMetadataSource;
assert pkgs.lib.hasInfix "nixfied-kernel workflow resolve-mode" runtimeMetadataSource;
assert pkgs.lib.hasInfix "_nixfied_task_load_cache() {" runtimeMetadataSource;
assert pkgs.lib.hasInfix "_nixfied_workflow_load_cache() {" runtimeMetadataSource;
assert pkgs.lib.hasInfix "task_runtime_plan_shell() {" runtimeMetadataSource;
assert pkgs.lib.hasInfix "task_hook_runtime_plan_shell() {" runtimeMetadataSource;
assert pkgs.lib.hasInfix "workflow_plan_task_ids() {" runtimeMetadataSource;
assert pkgs.lib.hasInfix "workflow_phase_tasks() {" runtimeMetadataSource;
assert pkgs.lib.hasInfix "workflow_parallel_enabled() {" runtimeMetadataSource;
assert pkgs.lib.hasInfix "workflow_write_summary() {" runtimeMetadataSource;
assert (!pkgs.lib.hasInfix "case \"$task_id\"" runtimeMetadataSource);
assert (!pkgs.lib.hasInfix "case \"$workflow_id\"" runtimeMetadataSource);
assert (!pkgs.lib.hasInfix "workflowPlanCases =" runtimeMetadataSource);
assert (!pkgs.lib.hasInfix "taskHookCases =" runtimeMetadataSource);
pkgs.runCommand "workflow-modes-contract" { } ''
  echo "OK: runtime metadata shell adapter replaces generated workflow/task case tables" > "$out"
''
