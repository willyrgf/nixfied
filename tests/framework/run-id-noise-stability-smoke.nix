{
  pkgs,
  registry,
}:
let
  frameworkLib = import ../../nixfied/framework/core {
    inherit pkgs;
    system = pkgs.system;
  };
  shellHelpers = import ./lib/shell-helpers.nix { inherit pkgs; };

  taskId = "task.test.run-id.noise";

  compiled = frameworkLib.mkNixfied {
    projectRoot = ../..;
    projectModules = [ ../../nixfied/project/module.nix ];
    extraModules = [
      (
        { ... }:
        {
          nixfied.tasks."test.run-id.noise" = {
            id = taskId;
            summary = "Run id noise probe";
            description = "Used to prove irrelevant ambient env does not change run ids.";
            commandApi.commandClass = "passthrough";
            runner.command = ''
              set -euo pipefail
              printf '%s\n' "OK: run id noise probe"
            '';
          };
        }
      )
    ];
    localOverrides = [ ];
  };

  executor = import ../../nixfied/framework/runtime/executor.nix {
    inherit
      pkgs
      registry
      ;
    model = compiled.model;
    selectionIndex = compiled.selectionIndex;
    services = compiled.services;
    projectRoot = ../..;
  };
in
pkgs.runCommand "run-id-noise-stability-smoke" { } ''
  set -euo pipefail
  ${shellHelpers.shellPrelude}

  EXECUTOR="${executor}/bin/nixfied-executor"
  export REGISTRY_ROOT="$TMPDIR/registry"
  export CI_ARTIFACTS_DIR="$TMPDIR/artifacts"
  export NIXFIED_RUNTIME_DIR_SCOPE_OVERRIDE="$TMPDIR/runtime-scope"
  mkdir -p "$REGISTRY_ROOT" "$CI_ARTIFACTS_DIR" "$NIXFIED_RUNTIME_DIR_SCOPE_OVERRIDE"

  FOO=alpha "$EXECUTOR" run-task "${taskId}" --run-id-file "$TMPDIR/run-a.run-id" > "$TMPDIR/run-a.out" 2>&1
  FOO=beta "$EXECUTOR" run-task "${taskId}" --run-id-file "$TMPDIR/run-b.run-id" > "$TMPDIR/run-b.out" 2>&1

  run_a="$(read_trimmed_file "$TMPDIR/run-a.run-id")"
  run_b="$(read_trimmed_file "$TMPDIR/run-b.run-id")"
  if [ "$run_a" != "$run_b" ]; then
    printf 'run_a=%s\nrun_b=%s\n' "$run_a" "$run_b"
    fail "irrelevant ambient env should not change run id"
  fi

  echo "OK: run id ignores irrelevant ambient env noise" > "$out"
''
