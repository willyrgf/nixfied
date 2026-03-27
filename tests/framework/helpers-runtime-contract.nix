{ pkgs }:
let
  helpersSource = builtins.readFile ../../nixfied/framework/runtime/helpers/helpers.nix;
  fixturesDslSource = builtins.readFile ../../nixfied/framework/runtime/helpers/fixtures.nix;
  kernelExportRuntimeSource = builtins.readFile ../../nixfied/framework/runtime/helpers/kernel-export-runtime.nix;
  runtimeEventsSource = builtins.readFile ../../nixfied/framework/runtime/helpers/runtime-events.nix;
  summarySource = builtins.readFile ../../nixfied/framework/runtime/helpers/summary.nix;
in
assert pkgs.lib.hasInfix "import ./cleanup-runtime.nix" helpersSource;
assert pkgs.lib.hasInfix "import ./fixture-runtime.nix" helpersSource;
assert pkgs.lib.hasInfix "import ./kernel-export-runtime.nix" helpersSource;
assert pkgs.lib.hasInfix "nixfied_load_kernel_exports()" kernelExportRuntimeSource;
assert !(pkgs.lib.hasInfix "import ./service-policy.nix" helpersSource);
assert pkgs.lib.hasInfix "nixfied-kernel service-policy start-service" helpersSource;
assert !(pkgs.lib.hasInfix "nixfied_policy_" helpersSource);
assert pkgs.lib.hasInfix "import ./kernel-export-runtime.nix" fixturesDslSource;
assert !(pkgs.lib.hasInfix "import ./service-policy.nix" fixturesDslSource);
assert pkgs.lib.hasInfix "nixfied-kernel service-policy fixture-keep-running" fixturesDslSource;
assert !(pkgs.lib.hasInfix "nixfied_policy_" fixturesDslSource);
assert !(builtins.pathExists ../../nixfied/framework/runtime/helpers/service-policy.nix);
assert pkgs.lib.hasInfix "import ./kernel-export-runtime.nix" runtimeEventsSource;
assert pkgs.lib.hasInfix "nixfied-kernel service-policy runtime-event" runtimeEventsSource;
assert !(pkgs.lib.hasInfix "import ./service-policy.nix" runtimeEventsSource);
assert pkgs.lib.hasInfix "nixfied-kernel summary render-human" summarySource;
assert (!pkgs.lib.hasInfix "\${pkgs.jq}/bin/jq" runtimeEventsSource);
assert (!pkgs.lib.hasInfix "\${pkgs.jq}/bin/jq" summarySource);
pkgs.runCommand "helpers-runtime-contract" { } ''
  echo "OK: helper shells stay thin and delegate runtime policy and summary work to the kernel" > "$out"
''
