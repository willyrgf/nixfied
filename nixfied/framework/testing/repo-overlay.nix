{
  lib,
  pkgs,
  projectRoot,
  ...
}:
let
  ownerFile = "nixfied/framework/testing/repo-overlay.nix";
  workspaceMarker = import ../workspace-marker.nix;
  workspaceMarkerPresent = workspaceMarker.isPresent projectRoot;
  catalog = import ./catalog.nix;
  plainShellLogging = import ../core/plain-shell-logging.nix;
  shellCommon = import ../core/shell-common.nix { inherit pkgs; };

  summaryArg = {
    name = "summary";
    kind = "flag";
    long = "--summary";
    description = "Print compact summary output.";
  };

  modeArg = {
    name = "mode";
    kind = "option";
    long = "--mode";
    type = "enum";
    values = catalog.profileNames;
    description = "Select test mode.";
  };

  loggingArgs = [
    {
      name = "log-level";
      kind = "option";
      long = "--log-level";
      type = "enum";
      values = [
        "error"
        "warn"
        "info"
        "debug"
        "trace"
      ];
      description = "Override LOG_LEVEL for this run.";
    }
    {
      name = "output-mode";
      kind = "option";
      long = "--output-mode";
      type = "enum";
      values = [
        "stdout"
        "logs"
        "both"
      ];
      description = "Override OUTPUT_MODE for this run.";
    }
  ];

  workflowProbePhases = {
    preRun.serviceSets = [
      {
        serviceSetId = "service-set.default";
        operation = "ready";
      }
    ];
    postRun = {
      serviceSets = [
        {
          serviceSetId = "service-set.default";
          operation = "health";
        }
      ];
      alwaysRun = true;
    };
  };

  nonEmptyShardNames =
    profileName:
    builtins.filter (
      shardName: (catalog.profileShardChecks.${profileName}.${shardName} or [ ]) != [ ]
    ) catalog.order;

  checkRef = checkName: ".#checks.${pkgs.system}.${checkName}";

  renderCheckBuildCommand =
    checkNames:
    builtins.concatStringsSep " \\\n  " (
      map (checkName: lib.escapeShellArg (checkRef checkName)) checkNames
    );

  mkCheckTaskId = profileName: shardName: "task.test.framework.${profileName}.${shardName}";

  mkCheckTaskScript =
    profileName: shardName:
    let
      checkNames = catalog.profileShardChecks.${profileName}.${shardName} or [ ];
      scriptName = "nixfied-test-${profileName}-${shardName}";
    in
    pkgs.writeShellScriptBin scriptName ''
      set -euo pipefail
      ${plainShellLogging { }}
      ${shellCommon}

      log_info "running framework checks profile=${profileName} shard=${shardName} checks=${toString (builtins.length checkNames)}"
      ${pkgs.nix}/bin/nix build --no-link \
        ${renderCheckBuildCommand checkNames}
      log_ok "framework checks passed profile=${profileName} shard=${shardName}"
    '';

  mkCheckTask =
    profileName: shardName:
    let
      taskId = mkCheckTaskId profileName shardName;
      package = mkCheckTaskScript profileName shardName;
      command = "nixfied-test-${profileName}-${shardName}";
    in
    {
      name = "${profileName}-${shardName}";
      value = {
        id = taskId;
        kind = "internal";
        summary = "Framework ${shardName} checks (${profileName})";
        description = "Runs the ${shardName} framework checks selected by the ${profileName} profile.";
        runner = {
          type = "derivation";
          inherit
            package
            command
            ;
          workflowId = null;
        };
      };
    };

  profileTaskEntries = builtins.concatLists (
    map (
      profileName: map (shardName: mkCheckTask profileName shardName) (nonEmptyShardNames profileName)
    ) catalog.profileNames
  );

  selfhostTaskEntry = {
    name = "test-framework-selfhost";
    value = {
      id = "task.test.framework.selfhost";
      kind = "internal";
      summary = "Framework self-host smoke command";
      description = "Runs framework commands through the built executor from the current evaluation closure.";
      runner = {
        type = "shell";
        workflowId = null;
        package = null;
        command = ''
          set -euo pipefail
          logs_dir="$(mktemp -d "''${TMPDIR:-/tmp}/framework-selfhost.XXXXXX")"
          task_dev_log="$logs_dir/task-dev.log"
          workflow_log="$logs_dir/workflow-ci-basic.log"
          cleanup() {
            local rc=$?
            if [ "$rc" -eq 0 ]; then
              rm -rf "$logs_dir"
            else
              echo "ERROR: self-host logs preserved dir=$logs_dir"
              echo "INFO: self-host task.dev log path=$task_dev_log"
              echo "INFO: self-host workflow.ci.basic log path=$workflow_log"
            fi
            return "$rc"
          }
          trap cleanup EXIT
          if [ -z "''${NIXFIED_EXECUTOR_SELF:-}" ]; then
            echo "ERROR: NIXFIED_EXECUTOR_SELF is not set"
            exit 3
          fi
          echo "INFO: self-host smoke start"
          echo "INFO: self-host logs dir=$logs_dir"
          if NIXFIED_CALLER_PWD="$PWD" "$NIXFIED_EXECUTOR_SELF" run-task task.dev >"$task_dev_log" 2>&1; then
            echo "OK: self-host task.dev completed"
          else
            rc="$?"
            echo "ERROR: self-host task.dev failed rc=$rc"
            cat "$task_dev_log"
            exit "$rc"
          fi
          if NIXFIED_CALLER_PWD="$PWD" "$NIXFIED_EXECUTOR_SELF" run-workflow workflow.ci.basic --summary >"$workflow_log" 2>&1; then
            echo "OK: self-host workflow.ci.basic completed"
          else
            rc="$?"
            echo "ERROR: self-host workflow.ci.basic failed rc=$rc"
            cat "$workflow_log"
            exit "$rc"
          fi
          echo "OK: self-host smoke complete"
        '';
      };
      runtime = {
        runtimeInputs = [
          pkgs.coreutils
          pkgs.findutils
          pkgs.gnused
          pkgs.gnugrep
        ];
        references = {
          taskIds = [ "task.dev" ];
          workflowIds = [ "workflow.ci.basic" ];
        };
      };
    };
  };

  mkProfileWorkflow =
    profileName:
    let
      shardTaskIds = map (shardName: mkCheckTaskId profileName shardName) (
        nonEmptyShardNames profileName
      );
      fullTaskIds =
        shardTaskIds ++ lib.optionals (profileName == "full") [ "task.test.framework.selfhost" ];
      stages = map (taskId: [ taskId ]) fullTaskIds;
    in
    {
      name = "test-${profileName}";
      value = {
        id = "workflow.test.${profileName}";
        summary = "Framework test workflow (${profileName})";
        description = "Runs the framework test workflow for the ${profileName} profile.";
        mode = "test";
        maxWorkers = 1;
        units = { };
        inherit stages;
        preRun = {
          tasks = [ ];
          serviceSets = [ ];
        };
        postRun = {
          tasks = [ ];
          serviceSets = [ ];
          alwaysRun = true;
        };
        artifacts = {
          keepOnSuccess = false;
          keepOnFailure = true;
          writeSummary = true;
        };
        execution = {
          parallel = false;
          failFast = true;
          lockPolicy = "exclusive";
          emitRegistryEvents = true;
          ephemeral.enable = false;
        };
      };
    };

  selfhostWorkflowEntry = {
    name = "test-framework-selfhost";
    value = {
      id = "workflow.test.framework.selfhost";
      summary = "Framework self-host smoke workflow";
      description = "Runs internal self-host command through workflow orchestration.";
      mode = "custom";
      maxWorkers = 1;
      units = {
        main = {
          taskId = "task.test.framework.selfhost";
          needs = [ ];
          locks = [ ];
          when = {
            envEquals = { };
            envPresent = [ ];
          };
          skipIfMissingEnv = [ ];
        };
      };
      stages = [ ];
      inherit (workflowProbePhases) preRun;
      inherit (workflowProbePhases) postRun;
      artifacts = {
        root = "/tmp/ci-artifacts";
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
    };
  };

  profileWorkflowEntries = map mkProfileWorkflow catalog.profileNames;
in
{
  config = lib.mkIf workspaceMarkerPresent {
    nixfied.tasks = builtins.listToAttrs (profileTaskEntries ++ [ selfhostTaskEntry ]) // {
      test = {
        description = lib.mkForce "Run the framework repository test workflows.";
        runner = {
          type = lib.mkForce "workflowRef";
          workflowId = lib.mkForce "workflow.test.full";
        };
        commandApi = {
          summary = lib.mkForce "Run tests";
          details = lib.mkForce "Run the framework repository test workflows.";
          usage = lib.mkForce [
            "nix run .#test"
            "nix run .#test -- --mode feature-proof --summary"
            "nix run .#test -- --mode ci --summary"
            "nix run .#test -- --mode full --summary"
          ];
          args = lib.mkForce (
            [
              summaryArg
              modeArg
            ]
            ++ loggingArgs
          );
        };
        launcher = {
          enable = lib.mkForce true;
          appId = lib.mkForce "test";
          summary = lib.mkForce "Run tests";
          description = lib.mkForce "Run the framework repository test workflows.";
          usage = lib.mkForce [
            "nix run .#test"
            "nix run .#test -- --mode feature-proof --summary"
            "nix run .#test -- --mode ci --summary"
            "nix run .#test -- --mode full --summary"
          ];
          examples = lib.mkForce [
            "nix run .#test -- --mode feature-proof --summary"
            "nix run .#test -- --mode ci --summary"
            "nix run .#test -- --mode full --summary"
          ];
          ownerFile = lib.mkForce ownerFile;
        };
      };
    };

    nixfied.workflows = builtins.listToAttrs (profileWorkflowEntries ++ [ selfhostWorkflowEntry ]);
  };
}
