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
pkgs.runCommand "sensitive-pass-through-smoke" { } ''
  set -euo pipefail

  EXECUTOR="${executor}/bin/nixfied-executor"
  export REGISTRY_ROOT="$TMPDIR/registry"
  mkdir -p "$REGISTRY_ROOT"

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

  echo "OK: sensitive pass-through enforcement validated" > "$out"
''
