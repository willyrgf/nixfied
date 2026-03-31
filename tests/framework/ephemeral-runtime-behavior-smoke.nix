{
  pkgs,
  model,
  services,
  serviceDefinitions,
  registry,
}:
let
  harnessLib = import ./lib/harness.nix;
  baseTask = model.tasks."task.ci.quality";
  sourceFixture = ./fixtures/ephemeral-nix-source-fixture;
  envFileName = ".ephemeral-secret.env";

  mkProbeTask =
    {
      taskId,
      summary,
      description,
      command,
      runtimeExtra ? { },
    }:
    baseTask
    // {
      id = taskId;
      inherit summary description;
      runner = {
        type = "shell";
        inherit command;
        package = null;
        workflowId = null;
      };
      runtime = baseTask.runtime // runtimeExtra;
    };

  mkProbeWorkflow =
    {
      workflowId,
      taskId,
      summary,
      description,
    }:
    let
      unit = {
        inherit taskId;
        needs = [ ];
        locks = [ ];
        when = {
          envEquals = { };
          envPresent = [ ];
        };
        skipIfMissingEnv = [ ];
      };
    in
    {
      id = workflowId;
      inherit summary description;
      mode = "custom";
      maxWorkers = 1;
      units.probe = unit;
      stages = [ [ "probe" ] ];
      preRun.tasks = [ ];
      postRun = {
        tasks = [ ];
        alwaysRun = true;
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
        ephemeral.enable = true;
      };
      plan = [ ({ name = "probe"; } // unit) ];
    };

  mkHarness =
    {
      probeModel,
      projectRoot ? ../..,
    }:
    harnessLib {
      inherit
        pkgs
        services
        serviceDefinitions
        registry
        projectRoot
        ;
      model = probeModel;
    };

  trackedWorkflowId = "workflow.test.ephemeral.copy-mode.tracked";
  worktreeWorkflowId = "workflow.test.ephemeral.copy-mode.worktree";
  disabledEnvWorkflowId = "workflow.test.ephemeral.env-file.disabled";
  originalRootEnvWorkflowId = "workflow.test.ephemeral.env-file.original-root";
  nixSourceWorkflowId = "workflow.test.ephemeral.nix-source";
  registryWorkflowId = "workflow.test.ephemeral.registry-isolation";

  trackedModel = model // {
    runtime = model.runtime // {
      ephemeral = (model.runtime.ephemeral or { }) // {
        copyMode = "git-files";
        includeUntracked = false;
      };
    };
    tasks = model.tasks // {
      "task.test.ephemeral.copy-mode.tracked" = mkProbeTask {
        taskId = "task.test.ephemeral.copy-mode.tracked";
        summary = "ephemeral tracked-only copy probe";
        description = "Verifies tracked-only copy mode excludes untracked files.";
        command = ''
          set -euo pipefail
          echo "INFO: sandbox_pwd=$(pwd -P)"
          test -f tracked.txt
          test -f tracked-ignored.md
          test ! -e keep-untracked.txt
          test ! -e ignored.tmp
          test ! -e ignored-dir/file.txt
          echo "OK: copy mode probe expect_untracked=0"
        '';
      };
    };
    workflows = model.workflows // {
      ${trackedWorkflowId} = mkProbeWorkflow {
        workflowId = trackedWorkflowId;
        taskId = "task.test.ephemeral.copy-mode.tracked";
        summary = "ephemeral tracked-only copy probe";
        description = "Validates tracked-only ephemeral source materialization.";
      };
    };
  };

  worktreeModel = model // {
    runtime = model.runtime // {
      ephemeral = (model.runtime.ephemeral or { }) // {
        copyMode = "git-files";
        includeUntracked = true;
      };
    };
    tasks = model.tasks // {
      "task.test.ephemeral.copy-mode.worktree" = mkProbeTask {
        taskId = "task.test.ephemeral.copy-mode.worktree";
        summary = "ephemeral worktree copy probe";
        description = "Verifies explicit worktree copy mode includes untracked files.";
        command = ''
          set -euo pipefail
          echo "INFO: sandbox_pwd=$(pwd -P)"
          test -f tracked.txt
          test -f tracked-ignored.md
          test -f keep-untracked.txt
          test ! -e ignored.tmp
          test ! -e ignored-dir/file.txt
          echo "OK: copy mode probe expect_untracked=1"
        '';
      };
    };
    workflows = model.workflows // {
      ${worktreeWorkflowId} = mkProbeWorkflow {
        workflowId = worktreeWorkflowId;
        taskId = "task.test.ephemeral.copy-mode.worktree";
        summary = "ephemeral worktree copy probe";
        description = "Validates worktree ephemeral source materialization.";
      };
    };
  };

  disabledEnvModel = model // {
    runtime = model.runtime // {
      ephemeral = (model.runtime.ephemeral or { }) // {
        copyMode = "git-files";
        includeUntracked = false;
        envFileMode = "disabled";
        envFilePath = envFileName;
      };
    };
    tasks = model.tasks // {
      "task.test.ephemeral.env-file.disabled" = mkProbeTask {
        taskId = "task.test.ephemeral.env-file.disabled";
        summary = "ephemeral env file disabled probe";
        description = "Verifies host env import stays disabled by default.";
        command = ''
          set -euo pipefail
          echo "INFO: env_secret=''${EPHEMERAL_HOST_ENV_SECRET:-}"
          test -z "''${EPHEMERAL_HOST_ENV_SECRET:-}"
          echo "OK: env file probe expect_secret=0"
        '';
        runtimeExtra = {
          passThroughEnv = (baseTask.runtime.passThroughEnv or [ ]) ++ [ "EPHEMERAL_HOST_ENV_SECRET" ];
          allowSensitivePassThrough = true;
        };
      };
    };
    workflows = model.workflows // {
      ${disabledEnvWorkflowId} = mkProbeWorkflow {
        workflowId = disabledEnvWorkflowId;
        taskId = "task.test.ephemeral.env-file.disabled";
        summary = "ephemeral env file disabled probe";
        description = "Validates disabled host env import in ephemeral mode.";
      };
    };
  };

  originalRootEnvModel = model // {
    runtime = model.runtime // {
      ephemeral = (model.runtime.ephemeral or { }) // {
        copyMode = "git-files";
        includeUntracked = false;
        envFileMode = "original-root";
        envFilePath = envFileName;
      };
    };
    tasks = model.tasks // {
      "task.test.ephemeral.env-file.original-root" = mkProbeTask {
        taskId = "task.test.ephemeral.env-file.original-root";
        summary = "ephemeral env file original-root probe";
        description = "Verifies host env import can be requested explicitly.";
        command = ''
          set -euo pipefail
          echo "INFO: env_secret=''${EPHEMERAL_HOST_ENV_SECRET:-}"
          test "''${EPHEMERAL_HOST_ENV_SECRET:-}" = "from-host-env"
          echo "OK: env file probe expect_secret=1"
        '';
        runtimeExtra = {
          passThroughEnv = (baseTask.runtime.passThroughEnv or [ ]) ++ [ "EPHEMERAL_HOST_ENV_SECRET" ];
          allowSensitivePassThrough = true;
        };
      };
    };
    workflows = model.workflows // {
      ${originalRootEnvWorkflowId} = mkProbeWorkflow {
        workflowId = originalRootEnvWorkflowId;
        taskId = "task.test.ephemeral.env-file.original-root";
        summary = "ephemeral env file original-root probe";
        description = "Validates opt-in host env import in ephemeral mode.";
      };
    };
  };

  nixSourceModel = model // {
    tasks = model.tasks // {
      "task.test.ephemeral.nix-source" = mkProbeTask {
        taskId = "task.test.ephemeral.nix-source";
        summary = "ephemeral nix-source probe";
        description = "Verifies ephemeral execution uses the compiled source snapshot by default.";
        command = ''
          set -euo pipefail
          test "$(cat source-origin.txt)" = "store-snapshot"
          test -f kept.txt
          test ! -e runtime-only.txt
          test ! -e node_modules/ignored.txt
          test ! -e build.log
          echo "OK: nix-source probe complete"
        '';
      };
    };
    workflows = model.workflows // {
      ${nixSourceWorkflowId} = mkProbeWorkflow {
        workflowId = nixSourceWorkflowId;
        taskId = "task.test.ephemeral.nix-source";
        summary = "ephemeral nix-source probe";
        description = "Validates deterministic nix-source materialization.";
      };
    };
  };

  registryModel = model // {
    runtime = model.runtime // {
      ephemeral = (model.runtime.ephemeral or { }) // {
        keepFailures = true;
        maxFailedRoots = 16;
      };
    };
    tasks = model.tasks // {
      "task.test.ephemeral.registry-isolation" = mkProbeTask {
        taskId = "task.test.ephemeral.registry-isolation";
        summary = "ephemeral registry isolation probe";
        description = "Fails intentionally so preserved ephemeral roots can be inspected.";
        command = ''
          set -euo pipefail
          printf '%s\n' "$REGISTRY_ROOT" > "$CI_ARTIFACTS_DIR/registry-root.txt"
          if [ -n "''${NIXFIED_EPHEMERAL_GATE_DIR:-}" ]; then
            ready_file="$NIXFIED_EPHEMERAL_GATE_DIR/$NIXFIED_RUN_ID.ready"
            release_file="$NIXFIED_EPHEMERAL_GATE_DIR/release"
            mkdir -p "$NIXFIED_EPHEMERAL_GATE_DIR"
            : > "$ready_file"
            while [ ! -f "$release_file" ]; do
              sleep 0.1
            done
          fi
          echo "ERROR: intentional failure to preserve ephemeral state" >&2
          exit 1
        '';
        runtimeExtra.passThroughEnv = (baseTask.runtime.passThroughEnv or [ ]) ++ [
          "NIXFIED_EPHEMERAL_GATE_DIR"
        ];
      };
    };
    workflows = model.workflows // {
      ${registryWorkflowId} = mkProbeWorkflow {
        workflowId = registryWorkflowId;
        taskId = "task.test.ephemeral.registry-isolation";
        summary = "ephemeral registry isolation probe";
        description = "Validates run-local registry roots for concurrent ephemeral runs.";
      };
    };
  };

  trackedHarness = mkHarness { probeModel = trackedModel; };
  worktreeHarness = mkHarness { probeModel = worktreeModel; };
  disabledEnvHarness = mkHarness { probeModel = disabledEnvModel; };
  originalRootEnvHarness = mkHarness { probeModel = originalRootEnvModel; };
  nixSourceHarness = mkHarness {
    probeModel = nixSourceModel;
    projectRoot = sourceFixture;
  };
  registryHarness = mkHarness { probeModel = registryModel; };
in
pkgs.runCommand "ephemeral-runtime-behavior-smoke" { } ''
  set -euo pipefail
  ${registryHarness.shellPrelude}

  run_success() {
    local label="$1"
    local out_file="$2"
    shift 2

    set +e
    "$@" > "$out_file" 2>&1
    rc="$?"
    set -e
    if [ "$rc" -ne 0 ]; then
      echo "workflow failed label=$label rc=$rc"
      cat "$out_file"
      exit 1
    fi
  }

  export REGISTRY_ROOT="$TMPDIR/registry"
  mkdir -p "$REGISTRY_ROOT"

  repo="$TMPDIR/repo"
  mkdir -p "$repo/subdir" "$repo/ignored-dir"
  printf 'ignored.tmp\nignored-dir/\n*.md\n!tracked-ignored.md\n' > "$repo/.gitignore"
  printf 'tracked\n' > "$repo/tracked.txt"
  printf 'tracked ignored\n' > "$repo/tracked-ignored.md"
  printf 'keep me\n' > "$repo/keep-untracked.txt"
  printf 'ignore me\n' > "$repo/ignored.tmp"
  printf 'ignore dir\n' > "$repo/ignored-dir/file.txt"
  printf 'EPHEMERAL_HOST_ENV_SECRET=from-host-env\n' > "$repo/${envFileName}"
  ${pkgs.git}/bin/git -C "$repo" init >/dev/null 2>&1
  ${pkgs.git}/bin/git -C "$repo" add .gitignore tracked.txt tracked-ignored.md

  TRACKED_ORCH="${trackedHarness.orchestrator}/bin/nixfied-orchestrator"
  WORKTREE_ORCH="${worktreeHarness.orchestrator}/bin/nixfied-orchestrator"
  DISABLED_ENV_ORCH="${disabledEnvHarness.orchestrator}/bin/nixfied-orchestrator"
  ORIGINAL_ROOT_ENV_ORCH="${originalRootEnvHarness.orchestrator}/bin/nixfied-orchestrator"
  NIX_SOURCE_ORCH="${nixSourceHarness.orchestrator}/bin/nixfied-orchestrator"
  REGISTRY_ORCH="${registryHarness.orchestrator}/bin/nixfied-orchestrator"

  run_success tracked-only "$TMPDIR/tracked.out" env NIXFIED_CALLER_PWD="$repo/subdir" "$TRACKED_ORCH" run-workflow "${trackedWorkflowId}" --summary
  run_success worktree "$TMPDIR/worktree.out" env NIXFIED_CALLER_PWD="$repo/subdir" "$WORKTREE_ORCH" run-workflow "${worktreeWorkflowId}" --summary
  run_success env-disabled "$TMPDIR/env-disabled.out" env NIXFIED_CALLER_PWD="$repo/subdir" "$DISABLED_ENV_ORCH" run-workflow "${disabledEnvWorkflowId}" --summary
  run_success env-original-root "$TMPDIR/env-original-root.out" env NIXFIED_CALLER_PWD="$repo/subdir" "$ORIGINAL_ROOT_ENV_ORCH" run-workflow "${originalRootEnvWorkflowId}" --summary

  sandbox_pwd="$(${pkgs.gnused}/bin/sed -n 's/^INFO: sandbox_pwd=//p' "$TMPDIR/tracked.out" | ${pkgs.coreutils}/bin/tail -n 1)"
  printf '%s\n' "$sandbox_pwd" | ${pkgs.gnugrep}/bin/grep -Eq '.+-ephemeral-.+/source$'
  ${pkgs.gnugrep}/bin/grep -Fq "INFO: Using git-files copy mode include_untracked=0" "$TMPDIR/tracked.out"
  ${pkgs.gnugrep}/bin/grep -Fq "OK: copy mode probe expect_untracked=0" "$TMPDIR/tracked.out"
  ${pkgs.gnugrep}/bin/grep -Fq "INFO: Using git-files copy mode include_untracked=1" "$TMPDIR/worktree.out"
  ${pkgs.gnugrep}/bin/grep -Fq "OK: copy mode probe expect_untracked=1" "$TMPDIR/worktree.out"
  ${pkgs.gnugrep}/bin/grep -Fq "INFO: Skipping host env file import mode=disabled" "$TMPDIR/env-disabled.out"
  ${pkgs.gnugrep}/bin/grep -Fq "OK: env file probe expect_secret=0" "$TMPDIR/env-disabled.out"
  ${pkgs.gnugrep}/bin/grep -Fq "INFO: Loading host env file mode=original-root path=$repo/${envFileName}" "$TMPDIR/env-original-root.out"
  ${pkgs.gnugrep}/bin/grep -Fq "OK: env file probe expect_secret=1" "$TMPDIR/env-original-root.out"

  source_repo="$TMPDIR/source-repo"
  mkdir -p "$source_repo/subdir"
  printf 'caller-root\n' > "$source_repo/source-origin.txt"
  printf 'runtime-only\n' > "$source_repo/runtime-only.txt"
  run_success nix-source "$TMPDIR/nix-source.out" env NIXFIED_CALLER_PWD="$source_repo/subdir" "$NIX_SOURCE_ORCH" run-workflow "${nixSourceWorkflowId}" --summary
  ${pkgs.gnugrep}/bin/grep -Fq "INFO: Using nix-source copy mode" "$TMPDIR/nix-source.out"
  ${pkgs.gnugrep}/bin/grep -Fq "OK: nix-source probe complete" "$TMPDIR/nix-source.out"

  export NIXFIED_EPHEMERAL_ROOT_BASE="$TMPDIR/ephemeral-roots"
  gate_dir="$TMPDIR/gates"
  mkdir -p "$NIXFIED_EPHEMERAL_ROOT_BASE" "$gate_dir"
  ${pkgs.git}/bin/git -C "$repo" -c user.name=nixfied -c user.email=nixfied@example.invalid \
    commit -m "init ephemeral runtime behavior probe repo" >/dev/null 2>&1

  set +e
  env NIXFIED_CALLER_PWD="$repo/subdir" NIXFIED_EPHEMERAL_GATE_DIR="$gate_dir" \
    "$REGISTRY_ORCH" run-workflow "${registryWorkflowId}" --run-id-file "$TMPDIR/run-1.run-id" --summary > "$TMPDIR/run-1.out" 2>&1 &
  pid_one="$!"
  env NIXFIED_CALLER_PWD="$repo/subdir" NIXFIED_EPHEMERAL_GATE_DIR="$gate_dir" \
    "$REGISTRY_ORCH" run-workflow "${registryWorkflowId}" --run-id-file "$TMPDIR/run-2.run-id" --summary > "$TMPDIR/run-2.out" 2>&1 &
  pid_two="$!"
  wait_for_condition 30 "run one id" test -s "$TMPDIR/run-1.run-id"
  wait_for_condition 30 "run two id" test -s "$TMPDIR/run-2.run-id"
  run_one="$(read_trimmed_file "$TMPDIR/run-1.run-id")"
  run_two="$(read_trimmed_file "$TMPDIR/run-2.run-id")"
  wait_for_condition 60 "run one ready" test -f "$gate_dir/$run_one.ready"
  wait_for_condition 60 "run two ready" test -f "$gate_dir/$run_two.ready"
  : > "$gate_dir/release"
  wait "$pid_one"
  rc_one="$?"
  wait "$pid_two"
  rc_two="$?"
  set -e

  if [ "$rc_one" -eq 0 ] || [ "$rc_two" -eq 0 ]; then
    fail "expected registry isolation probes to fail so ephemeral roots are preserved"
  fi

  root_one="$(${pkgs.gnused}/bin/sed -n 's/^INFO: Root: //p' "$TMPDIR/run-1.out" | ${pkgs.coreutils}/bin/tail -n 1)"
  root_two="$(${pkgs.gnused}/bin/sed -n 's/^INFO: Root: //p' "$TMPDIR/run-2.out" | ${pkgs.coreutils}/bin/tail -n 1)"
  require_non_empty "$root_one" "root_one"
  require_non_empty "$root_two" "root_two"
  if [ "$root_one" = "$root_two" ]; then
    fail "expected concurrent ephemeral runs to preserve distinct roots"
  fi

  mapfile -t preserved_roots < <(
    ${pkgs.findutils}/bin/find "$NIXFIED_EPHEMERAL_ROOT_BASE" -mindepth 1 -maxdepth 1 -type d -name "${model.identity.projectId}-ephemeral-failed-*" |
      ${pkgs.coreutils}/bin/sort
  )
  if [ "''${#preserved_roots[@]}" != "2" ]; then
    fail "expected exactly two preserved ephemeral roots"
  fi

  for root in "''${preserved_roots[@]}"; do
    require_file "$root/registry/events.ndjson"
    find "$root/artifacts" -type f -name registry-root.txt | ${pkgs.gnugrep}/bin/grep -q .
    find "$root/artifacts" -type f -name summary.json | ${pkgs.gnugrep}/bin/grep -q .
  done

  echo "OK: ephemeral runtime behavior stays scoped across source copy, env import, nix-source, and per-run registry roots" > "$out"
''
