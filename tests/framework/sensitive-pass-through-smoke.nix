{
  pkgs,
  registry,
}:
let
  frameworkLib = import ../../nixfied/framework/core {
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
          nixfied.tasks.sensitive-pass-through-blocked = {
            id = "task.test.sensitive.blocked";
            kind = "internal";
            summary = "Sensitive pass-through blocked task";
            runner = {
              type = "shell";
              command = ''
                set -euo pipefail
                echo "OK: blocked task command should not execute"
              '';
            };
            runtime.passThroughEnv = [ "API_KEY" ];
          };

          nixfied.tasks.sensitive-pass-through-allowed = {
            id = "task.test.sensitive.allowed";
            kind = "internal";
            summary = "Sensitive pass-through allowed task";
            runner = {
              type = "shell";
              command = ''
                set -euo pipefail
                if [ -z "''${API_KEY:-}" ]; then
                  echo "ERROR: API_KEY missing inside allowed task"
                  exit 1
                fi
                echo "OK: allowed task received API_KEY"
              '';
            };
            runtime.passThroughEnv = [ "API_KEY" ];
            runtime.allowSensitivePassThrough = true;
          };

          nixfied.tasks.runtime-owned-pass-through-blocked = {
            id = "task.test.runtime-owned.blocked";
            kind = "internal";
            summary = "Runtime-owned pass-through blocked task";
            runner = {
              type = "shell";
              command = ''
                set -euo pipefail
                echo "OK: runtime-owned blocked task command should not execute"
              '';
            };
            runtime.passThroughEnv = [ "HOME" ];
          };

          nixfied.tasks.runtime-owned-env-override-blocked = {
            id = "task.test.runtime-owned.env-override";
            kind = "internal";
            summary = "Runtime-owned env override blocked task";
            runner = {
              type = "shell";
              command = ''
                set -euo pipefail
                echo "OK: runtime-owned env override task command should not execute"
              '';
            };
            runtime.env = {
              XDG_CACHE_HOME = "/tmp/runtime-owned-override";
            };
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
    services = compiled.services;
    projectRoot = ../..;
  };
in
pkgs.runCommand "sensitive-pass-through-smoke" { } ''
  set -euo pipefail

  EXECUTOR="${executor}/bin/nixfied-executor"
  export REGISTRY_ROOT="$TMPDIR/registry"
  export NIXFIED_RUNTIME_DIR_SCOPE_OVERRIDE="$TMPDIR/runtime-scope"
  mkdir -p "$REGISTRY_ROOT"
  mkdir -p "$NIXFIED_RUNTIME_DIR_SCOPE_OVERRIDE"

  set +e
  API_KEY=top-secret "$EXECUTOR" run-task task.test.sensitive.blocked > "$TMPDIR/blocked.out" 2>&1
  blocked_rc="$?"
  set -e
  if [ "$blocked_rc" -eq 0 ]; then
    echo "expected sensitive pass-through to be blocked without opt-in"
    cat "$TMPDIR/blocked.out"
    exit 1
  fi
  if ! ${pkgs.gnugrep}/bin/grep -Fq "ERROR: sensitive passthrough env blocked name=API_KEY" "$TMPDIR/blocked.out"; then
    echo "missing blocked passthrough marker"
    cat "$TMPDIR/blocked.out"
    exit 1
  fi

  API_KEY=top-secret "$EXECUTOR" run-task task.test.sensitive.allowed > "$TMPDIR/allowed.out" 2>&1
  if ! ${pkgs.gnugrep}/bin/grep -Fq "OK: allowed task received API_KEY" "$TMPDIR/allowed.out"; then
    echo "allowed task did not receive API_KEY"
    cat "$TMPDIR/allowed.out"
    exit 1
  fi

  set +e
  HOME="$TMPDIR/host-home" "$EXECUTOR" run-task task.test.runtime-owned.blocked > "$TMPDIR/runtime-owned-blocked.out" 2>&1
  runtime_owned_rc="$?"
  set -e
  if [ "$runtime_owned_rc" -eq 0 ]; then
    echo "expected runtime-owned pass-through to be blocked"
    cat "$TMPDIR/runtime-owned-blocked.out"
    exit 1
  fi
  if ! ${pkgs.gnugrep}/bin/grep -Fq "ERROR: runtime-owned passthrough env blocked name=HOME" "$TMPDIR/runtime-owned-blocked.out"; then
    echo "missing runtime-owned passthrough marker"
    cat "$TMPDIR/runtime-owned-blocked.out"
    exit 1
  fi

  set +e
  "$EXECUTOR" run-task task.test.runtime-owned.env-override > "$TMPDIR/runtime-owned-override.out" 2>&1
  runtime_owned_override_rc="$?"
  set -e
  if [ "$runtime_owned_override_rc" -eq 0 ]; then
    echo "expected runtime-owned env override to be blocked"
    cat "$TMPDIR/runtime-owned-override.out"
    exit 1
  fi
  if ! ${pkgs.gnugrep}/bin/grep -Fq "ERROR: runtime-owned env override blocked name=XDG_CACHE_HOME" "$TMPDIR/runtime-owned-override.out"; then
    echo "missing runtime-owned env override marker"
    cat "$TMPDIR/runtime-owned-override.out"
    exit 1
  fi

  echo "OK: sensitive and runtime-owned pass-through enforcement validated" > "$out"
''
