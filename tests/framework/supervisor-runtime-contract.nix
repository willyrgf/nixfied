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
assert pkgs.lib.hasInfix "helperPrelude" runtimeSource;
assert pkgs.lib.hasInfix "configPrelude" runtimeSource;
assert pkgs.lib.hasInfix "supervisor_pid_file()" runtimeSource;
assert pkgs.lib.hasInfix "supervisor_process_api_ready()" runtimeSource;
assert pkgs.lib.hasInfix "supervisor_fetch_process_json()" runtimeSource;
assert pkgs.lib.hasInfix "supervisor_clear_state()" runtimeSource;
assert pkgs.lib.hasInfix "runtime = import ./runtime.nix" defaultSource;
assert pkgs.lib.hasInfix "runtime.mkSupervisorScript" lifecycleSource;
assert pkgs.lib.hasInfix "runtime.mkSupervisorScript" statusSource;
assert pkgs.lib.hasInfix "runtime.mkSupervisorScript" managementSource;
assert pkgs.lib.hasInfix "supervisor_clear_state" lifecycleSource;
assert pkgs.lib.hasInfix "supervisor_process_api_ready" lifecycleSource;
assert pkgs.lib.hasInfix "supervisor_process_api_ready" statusSource;
assert pkgs.lib.hasInfix "supervisor_fetch_process_json" statusSource;
pkgs.runCommand "supervisor-runtime-contract" { } ''
  echo "OK: supervisor runtime helper is wired into supervisor lifecycle/status/management" > "$out"
''
