{ pkgs }:
let
  helpersSource = builtins.readFile ../../nixfied/.framework/lib/helpers.nix;
  cleanupSource = builtins.readFile ../../nixfied/.framework/lib/cleanup-runtime.nix;
  fixtureSource = builtins.readFile ../../nixfied/.framework/lib/fixture-runtime.nix;
in
assert pkgs.lib.hasInfix "import ./cleanup-runtime.nix" helpersSource;
assert pkgs.lib.hasInfix "import ./fixture-runtime.nix" helpersSource;
assert pkgs.lib.hasInfix "cleanupRuntime.cleanupRuntime" helpersSource;
assert pkgs.lib.hasInfix "fixtureRuntime.fixtureRuntime" helpersSource;
assert pkgs.lib.hasInfix "with_cleanup()" cleanupSource;
assert pkgs.lib.hasInfix "_run_cleanups()" cleanupSource;
assert !(pkgs.lib.hasInfix "mktemp \"''${"TMPDIR:-/tmp"}/nixfied-cleanup" cleanupSource);
assert pkgs.lib.hasInfix "printf -v action '%q '" cleanupSource;
assert pkgs.lib.hasInfix "fixture_start_service()" fixtureSource;
assert pkgs.lib.hasInfix "_service_hook_name()" fixtureSource;
pkgs.runCommand "helpers-runtime-contract" { } ''
  echo "OK: helpers runtime responsibilities are split and cleanup stays in memory" > "$out"
''
