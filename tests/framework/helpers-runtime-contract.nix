{ pkgs }:
let
  helpersSource = builtins.readFile ../../nixfied/framework/runtime/helpers/helpers.nix;
  cleanupSource = builtins.readFile ../../nixfied/framework/runtime/helpers/cleanup-runtime.nix;
  fixtureSource = builtins.readFile ../../nixfied/framework/runtime/helpers/fixture-runtime.nix;
  fixturesDslSource = builtins.readFile ../../nixfied/framework/runtime/helpers/fixtures.nix;
  loggingRuntimeSource = builtins.readFile ../../nixfied/framework/runtime/helpers/logging-runtime.nix;
  runtimeEventsSource = builtins.readFile ../../nixfied/framework/runtime/helpers/runtime-events.nix;
  shellCommonSource = builtins.readFile ../../nixfied/framework/core/shell-common.nix;
  summarySource = builtins.readFile ../../nixfied/framework/runtime/helpers/summary.nix;
  runtimeDefaultsSource = builtins.readFile ../../nixfied/framework/core/runtime-defaults.nix;
in
assert pkgs.lib.hasInfix "import ./cleanup-runtime.nix" helpersSource;
assert pkgs.lib.hasInfix "import ./fixture-runtime.nix" helpersSource;
assert pkgs.lib.hasInfix "cleanupRuntime.cleanupRuntime" helpersSource;
assert pkgs.lib.hasInfix "fixtureRuntime.fixtureRuntime" helpersSource;
assert (!pkgs.lib.hasInfix "print_log_tail()" helpersSource);
assert pkgs.lib.hasInfix "nc -z \"$NIXFIED_LOCALHOST_NAME\" \"$port\"" helpersSource;
assert pkgs.lib.hasInfix "print_log_tail()" loggingRuntimeSource;
assert pkgs.lib.hasInfix "with_cleanup()" cleanupSource;
assert pkgs.lib.hasInfix "_run_cleanups()" cleanupSource;
assert !(pkgs.lib.hasInfix "mktemp \"''${"TMPDIR:-/tmp"}/nixfied-cleanup" cleanupSource);
assert pkgs.lib.hasInfix "printf -v action '%q '" cleanupSource;
assert pkgs.lib.hasInfix "fixture_start_service()" fixtureSource;
assert pkgs.lib.hasInfix "_service_hook_name()" fixtureSource;
assert pkgs.lib.hasInfix "runtimeDefaults = import ../../core/runtime-defaults.nix;"
  fixturesDslSource;
assert pkgs.lib.hasInfix "mkLocalPostgresUrlExportFromPortVar" fixturesDslSource;
assert pkgs.lib.hasInfix "mkLocalHttpUrlExportFromPortVar" fixturesDslSource;
assert pkgs.lib.hasInfix "NIXFIED_EXIT_USAGE=" shellCommonSource;
assert pkgs.lib.hasInfix "nixfied_require_next_arg()" shellCommonSource;
assert pkgs.lib.hasInfix "nixfied_exit_usage_with_usage()" shellCommonSource;
assert pkgs.lib.hasInfix "runtimeDefaults ? import ./runtime-defaults.nix" shellCommonSource;
assert pkgs.lib.hasInfix "slot_events_index_file_for()" runtimeEventsSource;
assert pkgs.lib.hasInfix "load_runtime_status_exports()" runtimeEventsSource;
assert pkgs.lib.hasInfix "load_runtime_event_policy_exports()" runtimeEventsSource;
assert pkgs.lib.hasInfix "nixfied-kernel service-policy runtime-event" runtimeEventsSource;
assert !(pkgs.lib.hasInfix "import ./service-policy.nix" runtimeEventsSource);
assert !(pkgs.lib.hasInfix "service_status_file_for()" runtimeEventsSource);
assert pkgs.lib.hasInfix "nixfied-kernel summary render-human" summarySource;
assert (!pkgs.lib.hasInfix "\${pkgs.jq}/bin/jq" runtimeEventsSource);
assert (!pkgs.lib.hasInfix "\${pkgs.jq}/bin/jq" summarySource);
assert pkgs.lib.hasInfix "loopbackIp = \"127.0.0.1\";" runtimeDefaultsSource;
assert pkgs.lib.hasInfix "extendedWaitIntervalSeconds = \"0.25\";" runtimeDefaultsSource;
assert pkgs.lib.hasInfix "import ../../core/shell-common.nix" loggingRuntimeSource;
pkgs.runCommand "helpers-runtime-contract" { } ''
  echo "OK: helpers runtime responsibilities are split and cleanup stays in memory" > "$out"
''
