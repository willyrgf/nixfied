{ pkgs }:
let
  runtimeSource = builtins.readFile ../../nixfied/framework/runtime/executor-runtime.nix;
in
assert !(builtins.pathExists ../../nixfied/framework/runtime/workflow-modes.nix);
assert !(builtins.pathExists ../../nixfied/framework/runtime/runtime-metadata.nix);
assert pkgs.lib.hasInfix "workflow_family_from_id() {" runtimeSource;
assert pkgs.lib.hasInfix "workflow_family_modes_joined() {" runtimeSource;
assert pkgs.lib.hasInfix "workflow_simple_shorthand_exists_for_family() {" runtimeSource;
assert pkgs.lib.hasInfix "workflow_resolve_mode_id() {" runtimeSource;
assert !(pkgs.lib.hasInfix "nixfied-kernel workflow resolve-mode" runtimeSource);
assert !(pkgs.lib.hasInfix "nixfied-kernel workflow load-runtime" runtimeSource);
assert !(pkgs.lib.hasInfix "nixfied-kernel task load-runtime" runtimeSource);
assert !(pkgs.lib.hasInfix "nixfied-kernel task load-hook" runtimeSource);
pkgs.runCommand "workflow-modes-contract" { } ''
  echo "OK: workflow mode resolution and descriptor lookup stay in runtime owners without a separate metadata layer" > "$out"
''
