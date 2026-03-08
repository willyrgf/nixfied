{ pkgs }:
let
  runtimeSource = builtins.readFile ../../nixfied/.framework/supervisor/runtime.nix;
  defaultSource = builtins.readFile ../../nixfied/.framework/supervisor/default.nix;
  lifecycleSource = builtins.readFile ../../nixfied/.framework/supervisor/lifecycle.nix;
  statusSource = builtins.readFile ../../nixfied/.framework/supervisor/status.nix;
  managementSource = builtins.readFile ../../nixfied/.framework/supervisor/management.nix;
in
assert pkgs.lib.hasInfix "mkSupervisorScript" runtimeSource;
assert pkgs.lib.hasInfix "slotPrelude" runtimeSource;
assert pkgs.lib.hasInfix "socketPrelude" runtimeSource;
assert pkgs.lib.hasInfix "configPrelude" runtimeSource;
assert pkgs.lib.hasInfix "runtime = import ./runtime.nix" defaultSource;
assert pkgs.lib.hasInfix "runtime.mkSupervisorScript" lifecycleSource;
assert pkgs.lib.hasInfix "runtime.mkSupervisorScript" statusSource;
assert pkgs.lib.hasInfix "runtime.mkSupervisorScript" managementSource;
pkgs.runCommand "supervisor-runtime-contract" { } ''
  echo "OK: supervisor runtime helper is wired into supervisor lifecycle/status/management" > "$out"
''
