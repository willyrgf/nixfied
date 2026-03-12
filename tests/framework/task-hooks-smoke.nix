{
  pkgs,
  model,
  registry,
}:
let
  baseTask = model.tasks."task.check";

  mkShellTask =
    {
      id,
      command,
      preHooks ? { },
      postHooks ? { },
    }:
    baseTask
    // {
      inherit id;
      summary = id;
      description = id;
      runner = {
        type = "shell";
        command = command;
        package = null;
        workflowId = null;
      };
      runtime = baseTask.runtime // {
        workdir = "stateRoot";
        customWorkdir = null;
        preHooks = preHooks;
        postHooks = postHooks;
      };
      ui = baseTask.ui // {
        app = baseTask.ui.app // {
          expose = false;
          name = builtins.replaceStrings [ "." ] [ "-" ] id;
        };
      };
    };

  hooksModel = model // {
    tasks = model.tasks // {
      "task.test.hooks.order" = mkShellTask {
        id = "task.test.hooks.order";
        command = ''
          set -euo pipefail
          echo "main" >> "./order.log"
        '';
        preHooks = {
          "01.first" = {
            command = ''
              set -euo pipefail
              echo "pre-1" >> "./order.log"
            '';
          };
          "02.second" = {
            command = ''
              set -euo pipefail
              echo "pre-2" >> "./order.log"
            '';
          };
        };
        postHooks = {
          "10.final" = {
            command = ''
              set -euo pipefail
              echo "post" >> "./order.log"
            '';
          };
        };
      };

      "task.test.hooks.main-fails" = mkShellTask {
        id = "task.test.hooks.main-fails";
        command = ''
          set -euo pipefail
          echo "main-fail" >> "./main-fails.log"
          exit 9
        '';
        postHooks = {
          "after.main" = {
            command = ''
              set -euo pipefail
              echo "post-after-main-fail" >> "./main-fails.log"
            '';
          };
        };
      };

      "task.test.hooks.post-fails" = mkShellTask {
        id = "task.test.hooks.post-fails";
        command = ''
          set -euo pipefail
          echo "main-ok" >> "./post-fails.log"
        '';
        postHooks = {
          "broken.post" = {
            command = ''
              set -euo pipefail
              echo "post-fail" >> "./post-fails.log"
              exit 17
            '';
          };
        };
      };

      "task.test.hooks.pre-fails" = mkShellTask {
        id = "task.test.hooks.pre-fails";
        command = ''
          set -euo pipefail
          echo "main-should-not-run" >> "./pre-fails.log"
        '';
        preHooks = {
          "broken.pre" = {
            command = ''
              set -euo pipefail
              echo "pre-fail" >> "./pre-fails.log"
              exit 13
            '';
          };
        };
        postHooks = {
          "should.not.run" = {
            command = ''
              set -euo pipefail
              echo "post-should-not-run" >> "./pre-fails.log"
            '';
          };
        };
      };

      "task.test.hooks.unsupported-runner" = baseTask // {
        id = "task.test.hooks.unsupported-runner";
        summary = "hooks unsupported runner";
        description = "hooks unsupported runner";
        runner = {
          type = "workflowRef";
          command = "";
          package = null;
          workflowId = "workflow.ci.full";
        };
        runtime = baseTask.runtime // {
          preHooks = {
            "pre.hook" = {
              command = ''
                set -euo pipefail
                echo "should-not-run"
              '';
            };
          };
          postHooks = { };
        };
        ui = baseTask.ui // {
          app = baseTask.ui.app // {
            expose = false;
            name = "task-test-hooks-unsupported-runner";
          };
        };
      };
    };
  };

  executor = import ../../nixfied/framework/runtime/executor.nix {
    inherit
      pkgs
      registry
      ;
    model = hooksModel;
    projectRoot = ../..;
  };

  frameworkLib = import ../../nixfied/lib {
    inherit pkgs;
    system = pkgs.system;
  };

  overrideCompiled = frameworkLib.mkNixfied {
    projectRoot = ../..;
    projectModules = [ ../../nixfied/project/module.nix ];
    extraModules = [
      {
        nixfied.tasks.format.runtime.postHooks."framework.nixfmt".command = "echo overridden-hook";
      }
    ];
    localOverrides = [ ];
  };
in
assert
  overrideCompiled.model.tasks."task.format".runtime.postHooks."framework.nixfmt".command
  == "echo overridden-hook";
pkgs.runCommand "task-hooks-smoke" { } ''
    set -euo pipefail

    EXECUTOR="${executor}/bin/nixfied-executor"
    runtime_scope="$TMPDIR/runtime-scope"
    runtime_registry="$runtime_scope/registry"
    export REGISTRY_ROOT="$TMPDIR/registry"
    export NIXFIED_RUNTIME_DIR_SCOPE_OVERRIDE="$runtime_scope"
    mkdir -p "$REGISTRY_ROOT"
    mkdir -p "$runtime_scope"

    set +e
    "$EXECUTOR" run-task task.test.hooks.order > "$TMPDIR/order.out" 2>&1
    order_rc="$?"
    set -e
    if [ "$order_rc" -ne 0 ]; then
      echo "expected hooks-order task to pass, got rc=$order_rc"
      cat "$TMPDIR/order.out"
      exit 1
    fi
    cat > "$TMPDIR/order.expected" <<'EOF'
  pre-1
  pre-2
  main
  post
  EOF
    if ! ${pkgs.diffutils}/bin/diff -u "$TMPDIR/order.expected" "$runtime_registry/order.log"; then
      echo "unexpected hook execution order"
      cat "$TMPDIR/order.out"
      exit 1
    fi

    set +e
    "$EXECUTOR" run-task task.test.hooks.main-fails > "$TMPDIR/main-fails.out" 2>&1
    main_fails_rc="$?"
    set -e
    if [ "$main_fails_rc" -eq 0 ]; then
      echo "expected main-fails task to fail"
      cat "$TMPDIR/main-fails.out"
      exit 1
    fi
    cat > "$TMPDIR/main-fails.expected" <<'EOF'
  main-fail
  post-after-main-fail
  EOF
    if ! ${pkgs.diffutils}/bin/diff -u "$TMPDIR/main-fails.expected" "$runtime_registry/main-fails.log"; then
      echo "post hook did not run after main failure"
      cat "$TMPDIR/main-fails.out"
      exit 1
    fi

    set +e
    "$EXECUTOR" run-task task.test.hooks.post-fails > "$TMPDIR/post-fails.out" 2>&1
    post_fails_rc="$?"
    set -e
    if [ "$post_fails_rc" -eq 0 ]; then
      echo "expected post-fails task to fail"
      cat "$TMPDIR/post-fails.out"
      exit 1
    fi
    if ! ${pkgs.gnugrep}/bin/grep -q "^main-ok$" "$runtime_registry/post-fails.log"; then
      echo "main command did not run for post-fails task"
      cat "$TMPDIR/post-fails.out"
      exit 1
    fi
    if ! ${pkgs.gnugrep}/bin/grep -q "^post-fail$" "$runtime_registry/post-fails.log"; then
      echo "post command did not run for post-fails task"
      cat "$TMPDIR/post-fails.out"
      exit 1
    fi

    set +e
    "$EXECUTOR" run-task task.test.hooks.pre-fails > "$TMPDIR/pre-fails.out" 2>&1
    pre_fails_rc="$?"
    set -e
    if [ "$pre_fails_rc" -eq 0 ]; then
      echo "expected pre-fails task to fail"
      cat "$TMPDIR/pre-fails.out"
      exit 1
    fi
    cat > "$TMPDIR/pre-fails.expected" <<'EOF'
  pre-fail
  EOF
    if ! ${pkgs.diffutils}/bin/diff -u "$TMPDIR/pre-fails.expected" "$runtime_registry/pre-fails.log"; then
      echo "main/post should not run after pre hook failure"
      cat "$TMPDIR/pre-fails.out"
      exit 1
    fi

    set +e
    "$EXECUTOR" run-task task.test.hooks.unsupported-runner > "$TMPDIR/unsupported.out" 2>&1
    unsupported_rc="$?"
    set -e
    if [ "$unsupported_rc" -eq 0 ]; then
      echo "expected unsupported-runner task to fail"
      cat "$TMPDIR/unsupported.out"
      exit 1
    fi
    if ! ${pkgs.gnugrep}/bin/grep -Fq "defines runtime hooks but runner type 'workflowRef' is unsupported" "$TMPDIR/unsupported.out"; then
      echo "missing unsupported runner error"
      cat "$TMPDIR/unsupported.out"
      exit 1
    fi

    echo "OK: task hooks behavior is validated" > "$out"
''
