{ pkgs }:
let
  runtimeSource = builtins.readFile ../../nixfied/framework/runtime/services/supervisor/runtime.nix;
  defaultSource = builtins.readFile ../../nixfied/framework/runtime/services/supervisor/default.nix;
  lifecycleSource = builtins.readFile ../../nixfied/framework/runtime/services/supervisor/lifecycle.nix;
  statusSource = builtins.readFile ../../nixfied/framework/runtime/services/supervisor/status.nix;
  managementSource = builtins.readFile ../../nixfied/framework/runtime/services/supervisor/management.nix;
in
assert pkgs.lib.hasInfix "mkSupervisorScript" runtimeSource;
assert pkgs.lib.hasInfix "slotPrelude" runtimeSource;
assert pkgs.lib.hasInfix "socketPrelude" runtimeSource;
assert pkgs.lib.hasInfix "helperPrelude" runtimeSource;
assert pkgs.lib.hasInfix "configPrelude" runtimeSource;
assert pkgs.lib.hasInfix "supervisor_pid_file()" runtimeSource;
assert pkgs.lib.hasInfix "supervisor_exec_up()" runtimeSource;
assert pkgs.lib.hasInfix "supervisor_stop_server()" runtimeSource;
assert pkgs.lib.hasInfix "supervisor_process_api_ready()" runtimeSource;
assert pkgs.lib.hasInfix "supervisor_fetch_process_json()" runtimeSource;
assert pkgs.lib.hasInfix "supervisor_spawn_daemon()" runtimeSource;
assert pkgs.lib.hasInfix "supervisor_wait_process_api_ready()" runtimeSource;
assert pkgs.lib.hasInfix "supervisor_cleanup_orphans()" runtimeSource;
assert pkgs.lib.hasInfix "supervisor_clear_state()" runtimeSource;
assert pkgs.lib.hasInfix "runtime = import ./runtime.nix" defaultSource;
assert pkgs.lib.hasInfix "runtime.mkSupervisorScript" lifecycleSource;
assert pkgs.lib.hasInfix "runtime.mkSupervisorScript" statusSource;
assert pkgs.lib.hasInfix "runtime.mkSupervisorScript" managementSource;
assert pkgs.lib.hasInfix "supervisor_exec_up" lifecycleSource;
assert pkgs.lib.hasInfix "supervisor_stop_server" lifecycleSource;
assert pkgs.lib.hasInfix "supervisor_spawn_daemon" lifecycleSource;
assert pkgs.lib.hasInfix "supervisor_wait_process_api_ready" lifecycleSource;
assert pkgs.lib.hasInfix "supervisor_cleanup_orphans" lifecycleSource;
assert pkgs.lib.hasInfix "supervisor_clear_state" lifecycleSource;
assert pkgs.lib.hasInfix "supervisor_process_api_ready" lifecycleSource;
assert pkgs.lib.hasInfix "supervisor_process_api_ready" statusSource;
assert pkgs.lib.hasInfix "supervisor_fetch_process_json" statusSource;
pkgs.runCommand "supervisor-runtime-contract" { } ''
  echo "OK: supervisor runtime helper is wired into supervisor lifecycle/status/management" > "$out"
''
