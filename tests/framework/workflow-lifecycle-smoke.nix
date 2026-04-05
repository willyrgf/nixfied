{
  pkgs,
  model,
  services,
  serviceDefinitions,
  registry,
}:
let
  runtimeFixture = import ./lib/runtime-fixture.nix { inherit pkgs; };
  baseTask = model.tasks."task.check";

  mkShellTask =
    {
      id,
      command,
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
        preHooks = { };
        postHooks = { };
      };
    };

  mkWorkflow =
    {
      id,
      mainTask,
      preTask,
      postTask,
      alwaysRun,
    }:
    {
      inherit id;
      summary = id;
      description = id;
      mode = "custom";
      maxWorkers = 1;
      units = {
        main = {
          taskId = mainTask;
          needs = [ ];
          locks = [ ];
          when = {
            envEquals = { };
            envPresent = [ ];
          };
          skipIfMissingEnv = [ ];
        };
      };
      stages = [ [ "main" ] ];
      preRun = {
        tasks = [ preTask ];
      };
      postRun = {
        tasks = [ postTask ];
        alwaysRun = alwaysRun;
      };
      artifacts = {
        root = "artifacts-root";
        keepOnSuccess = false;
        keepOnFailure = true;
        writeSummary = true;
      };
      execution = {
        parallel = false;
        failFast = true;
        lockPolicy = "exclusive";
        emitRegistryEvents = true;
        ephemeral = {
          enable = null;
        };
      };
      plan = [
        {
          name = "main";
          taskId = mainTask;
          needs = [ ];
          locks = [ ];
          when = {
            envEquals = { };
            envPresent = [ ];
          };
          skipIfMissingEnv = [ ];
        }
      ];
    };

  lifecycleModel = runtimeFixture.withCompiledExecution (
    model
    // {
      tasks = model.tasks // {
        "task.test.lifecycle.pre.always" = mkShellTask {
          id = "task.test.lifecycle.pre.always";
          command = ''
            set -euo pipefail
            echo "pre" >> "./lifecycle-always.log"
          '';
        };

        "task.test.lifecycle.main.always" = mkShellTask {
          id = "task.test.lifecycle.main.always";
          command = ''
            set -euo pipefail
            echo "main" >> "./lifecycle-always.log"
            exit 11
          '';
        };

        "task.test.lifecycle.post.always" = mkShellTask {
          id = "task.test.lifecycle.post.always";
          command = ''
            set -euo pipefail
            echo "post" >> "./lifecycle-always.log"
          '';
        };

        "task.test.lifecycle.pre.skip" = mkShellTask {
          id = "task.test.lifecycle.pre.skip";
          command = ''
            set -euo pipefail
            echo "pre" >> "./lifecycle-skip.log"
          '';
        };

        "task.test.lifecycle.main.skip" = mkShellTask {
          id = "task.test.lifecycle.main.skip";
          command = ''
            set -euo pipefail
            echo "main" >> "./lifecycle-skip.log"
            exit 12
          '';
        };

        "task.test.lifecycle.post.skip" = mkShellTask {
          id = "task.test.lifecycle.post.skip";
          command = ''
            set -euo pipefail
            echo "post" >> "./lifecycle-skip.log"
          '';
        };
      };

      workflows = model.workflows // {
        "workflow.test.lifecycle.always" = mkWorkflow {
          id = "workflow.test.lifecycle.always";
          mainTask = "task.test.lifecycle.main.always";
          preTask = "task.test.lifecycle.pre.always";
          postTask = "task.test.lifecycle.post.always";
          alwaysRun = true;
        };

        "workflow.test.lifecycle.skip" = mkWorkflow {
          id = "workflow.test.lifecycle.skip";
          mainTask = "task.test.lifecycle.main.skip";
          preTask = "task.test.lifecycle.pre.skip";
          postTask = "task.test.lifecycle.post.skip";
          alwaysRun = false;
        };
      };
    }
  );
  runtimeDeps = runtimeFixture.runtimeMaterialization {
    inherit
      pkgs
      services
      serviceDefinitions
      ;
    model = lifecycleModel;
  };

  executor = import ../../nixfied/framework/runtime/executor.nix {
    inherit
      pkgs
      registry
      ;
    model = lifecycleModel;
    inherit services;
    projectRoot = ../..;
    serviceDispatcherProgram = runtimeDeps.serviceDispatcherProgram;
    runtimeBin = runtimeDeps.runtimeEngineProgram;
  };
in
pkgs.runCommand "workflow-lifecycle-smoke" { } ''
    set -euo pipefail

    EXECUTOR="${executor}/bin/nixfied-executor"
    runtime_scope="$TMPDIR/runtime-scope"
    runtime_registry="$runtime_scope/registry"
    export REGISTRY_ROOT="$TMPDIR/registry"
    export NIXFIED_RUNTIME_DIR_SCOPE_OVERRIDE="$runtime_scope"
    mkdir -p "$REGISTRY_ROOT"
    mkdir -p "$runtime_scope"

    repo="$TMPDIR/repo"
    mkdir -p "$repo/subdir"
    printf 'tracked workflow lifecycle probe\n' > "$repo/tracked.txt"
    printf 'tracked subdir probe\n' > "$repo/subdir/probe.txt"
    ${pkgs.git}/bin/git -C "$repo" init >/dev/null 2>&1
    ${pkgs.git}/bin/git -C "$repo" add tracked.txt subdir/probe.txt
    ${pkgs.git}/bin/git -C "$repo" -c user.name=nixfied -c user.email=nixfied@example.invalid \
      commit -m "init workflow lifecycle probe repo" >/dev/null 2>&1

    set +e
    NIXFIED_CALLER_PWD="$repo/subdir" "$EXECUTOR" run-workflow workflow.test.lifecycle.always > "$TMPDIR/always.out" 2>&1
    rc_always="$?"
    set -e
    if [ "$rc_always" -eq 0 ]; then
      echo "expected workflow.test.lifecycle.always to fail"
      cat "$TMPDIR/always.out"
      exit 1
    fi

    cat > "$TMPDIR/always.expected" <<'EOF_ALWAYS'
  pre
  main
  post
  EOF_ALWAYS
    if ! ${pkgs.diffutils}/bin/diff -u "$TMPDIR/always.expected" "$runtime_registry/lifecycle-always.log"; then
      echo "unexpected lifecycle order for alwaysRun=true"
      cat "$TMPDIR/always.out"
      exit 1
    fi

    set +e
    NIXFIED_CALLER_PWD="$repo/subdir" "$EXECUTOR" run-workflow workflow.test.lifecycle.skip > "$TMPDIR/skip.out" 2>&1
    rc_skip="$?"
    set -e
    if [ "$rc_skip" -eq 0 ]; then
      echo "expected workflow.test.lifecycle.skip to fail"
      cat "$TMPDIR/skip.out"
      exit 1
    fi

    cat > "$TMPDIR/skip.expected" <<'EOF_SKIP'
  pre
  main
  EOF_SKIP
    if ! ${pkgs.diffutils}/bin/diff -u "$TMPDIR/skip.expected" "$runtime_registry/lifecycle-skip.log"; then
      echo "unexpected lifecycle order for alwaysRun=false"
      cat "$TMPDIR/skip.out"
      exit 1
    fi

    echo "OK: workflow preRun/postRun lifecycle is validated" > "$out"
''
