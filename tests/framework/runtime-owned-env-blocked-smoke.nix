{
  pkgs,
  registry,
}:
let
  frameworkLib = import ../../nixfied/lib {
    inherit pkgs;
    system = pkgs.system;
  };

  compiled = frameworkLib.mkNixfied {
    projectRoot = ../..;
    projectModules = [ ../../nixfied/project/module.nix ];
    extraModules = [
      (
        { ... }:
        {
          nixfied.tasks.runtime-owned-pass-through-blocked = {
            id = "task.test.runtime-owned.pass-through";
            kind = "internal";
            summary = "Runtime-owned pass-through blocked task";
            runner = {
              type = "shell";
              command = ''
                set -euo pipefail
                echo "OK: runtime-owned pass-through should not execute"
              '';
            };
            runtime.passThroughEnv = [ "HOME" ];
          };

          nixfied.tasks.runtime-owned-env-blocked = {
            id = "task.test.runtime-owned.env";
            kind = "internal";
            summary = "Runtime-owned env override blocked task";
            runner = {
              type = "shell";
              command = ''
                set -euo pipefail
                echo "OK: runtime-owned env override should not execute"
              '';
            };
            runtime.env = {
              HOME = "/tmp/forbidden-home";
            };
          };

          nixfied.tasks.runtime-owned-scope-pass-through-blocked = {
            id = "task.test.runtime-owned.scope-pass-through";
            kind = "internal";
            summary = "Runtime-owned scope pass-through blocked task";
            runner = {
              type = "shell";
              command = ''
                set -euo pipefail
                echo "OK: runtime-owned scope pass-through should not execute"
              '';
            };
            runtime.passThroughEnv = [ "NIXFIED_RUNTIME_DIR_SCOPE_OVERRIDE" ];
          };
        }
      )
    ];
    localOverrides = [ ];
  };

  executor = import ../../nixfied/runner/executor.nix {
    inherit
      pkgs
      registry
      ;
    model = compiled.model;
    projectRoot = ../..;
  };
in
pkgs.runCommand "runtime-owned-env-blocked-smoke" { } ''
  set -euo pipefail

  EXECUTOR="${executor}/bin/nixfied-executor"
  export REGISTRY_ROOT="$TMPDIR/registry"
  export NIXFIED_RUNTIME_DIR_SCOPE_OVERRIDE="$TMPDIR/runtime-scope"
  mkdir -p "$REGISTRY_ROOT"
  mkdir -p "$NIXFIED_RUNTIME_DIR_SCOPE_OVERRIDE"

  set +e
  HOME="$TMPDIR/host-home" "$EXECUTOR" run-task task.test.runtime-owned.pass-through > "$TMPDIR/pass-through.out" 2>&1
  pass_through_rc="$?"
  set -e
  if [ "$pass_through_rc" -eq 0 ]; then
    echo "expected runtime-owned pass-through to be blocked"
    cat "$TMPDIR/pass-through.out"
    exit 1
  fi
  if ! ${pkgs.gnugrep}/bin/grep -Fq "ERROR: runtime-owned passthrough env blocked name=HOME" "$TMPDIR/pass-through.out"; then
    echo "missing runtime-owned pass-through block marker"
    cat "$TMPDIR/pass-through.out"
    exit 1
  fi

  set +e
  "$EXECUTOR" run-task task.test.runtime-owned.env > "$TMPDIR/env.out" 2>&1
  env_rc="$?"
  set -e
  if [ "$env_rc" -eq 0 ]; then
    echo "expected runtime-owned env override to be blocked"
    cat "$TMPDIR/env.out"
    exit 1
  fi
  if ! ${pkgs.gnugrep}/bin/grep -Fq "ERROR: runtime-owned env override blocked name=HOME" "$TMPDIR/env.out"; then
    echo "missing runtime-owned env override block marker"
    cat "$TMPDIR/env.out"
    exit 1
  fi

  set +e
  "$EXECUTOR" run-task task.test.runtime-owned.scope-pass-through > "$TMPDIR/scope-pass-through.out" 2>&1
  scope_pass_through_rc="$?"
  set -e
  if [ "$scope_pass_through_rc" -eq 0 ]; then
    echo "expected runtime-owned scope pass-through to be blocked"
    cat "$TMPDIR/scope-pass-through.out"
    exit 1
  fi
  if ! ${pkgs.gnugrep}/bin/grep -Fq "ERROR: runtime-owned passthrough env blocked name=NIXFIED_RUNTIME_DIR_SCOPE_OVERRIDE" "$TMPDIR/scope-pass-through.out"; then
    echo "missing runtime-owned scope pass-through block marker"
    cat "$TMPDIR/scope-pass-through.out"
    exit 1
  fi

  echo "OK: runtime-owned env overrides are rejected" > "$out"
''
