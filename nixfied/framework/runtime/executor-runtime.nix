{
  pkgs,
  model ? null,
}:
let
  lib = pkgs.lib;
  commonRuntimeShell = import ./common-runtime.nix { inherit pkgs; };
  skipPolicy = import ./helpers/skip-policy.nix { inherit pkgs; };
  kernelPackage = import ./kernel { inherit pkgs; };
  runtimeMetadata =
    if model == null then
      {
        tasks = { };
        workflows = { };
        workflowFamilies = { };
      }
    else
      ((model.compiled or { }).runtimeMetadata or {
        tasks = { };
        workflows = { };
        workflowFamilies = { };
      });
  tasks = runtimeMetadata.tasks or { };
  workflows = runtimeMetadata.workflows or { };
  workflowFamilies = runtimeMetadata.workflowFamilies or { };
  taskIds = builtins.sort builtins.lessThan (builtins.attrNames tasks);
  workflowIds = builtins.sort builtins.lessThan (builtins.attrNames workflows);
  workflowFamilyIds = builtins.sort builtins.lessThan (builtins.attrNames workflowFamilies);

  taskEntries = map (
    taskId:
    let
      task = tasks.${taskId};
      runner = task.runner or { };
      deps = task.deps or { };
      hooks = task.hooks or { };
      help = task.help or { };
    in
    {
      key = taskId;
      value = {
        helpLines = help.lines or [ ];
        runnerType = runner.type or "shell";
        runnerCommand = runner.command or "";
        runnerPackage = runner.package or "";
        runnerWorkflowId = runner.workflowId or "";
        requiredServices = task.requiredServices or [ ];
        closureSelectedServices = task.closureSelectedServices or [ ];
        baseClosureSelectedServices = task.baseClosureSelectedServices or [ ];
        runtimePlanShell = task.runtimePlanShell or "";
        passThroughEnvNames = task.passThroughEnvNames or [ ];
        producesJson = builtins.toJSON (task.produces or { });
        maxAttempts = toString (task.maxAttempts or 1);
        retryBackoffValues = map toString (task.retryBackoffValues or [ ]);
        needs = deps.needs or [ ];
        softNeeds = deps.softNeeds or [ ];
        hookCount = toString (hooks.count or 0);
        preHookIds = hooks.preIds or [ ];
        postHookIds = hooks.postIds or [ ];
      };
    }
  ) taskIds;

  taskHookEntries = builtins.concatLists (
    map (
      taskId:
      let
        task = tasks.${taskId};
        hooks = task.hooks or { };
        mkPhaseEntries =
          phase: phaseHooks:
          map (
            hookId:
            let
              hook = phaseHooks.${hookId};
            in
            {
              key = "${taskId}:${phase}:${hookId}";
              value = {
                command = hook.command or "";
                runtimePlanShell = hook.runtimePlanShell or "";
                passThroughEnvNames = hook.passThroughEnvNames or [ ];
              };
            }
          ) (builtins.sort builtins.lessThan (builtins.attrNames phaseHooks));
      in
      (mkPhaseEntries "pre" (hooks.pre or { })) ++ (mkPhaseEntries "post" (hooks.post or { }))
    ) taskIds
  );

  taskHookIdEntries = builtins.concatLists (
    map (entry: [
      {
        key = "${entry.key}:pre";
        value = entry.value.preHookIds;
      }
      {
        key = "${entry.key}:post";
        value = entry.value.postHookIds;
      }
    ]) taskEntries
  );

  workflowEntries = map (
    workflowId:
    let
      workflow = workflows.${workflowId};
      logging = workflow.logging or { };
      phases = workflow.phases or { };
      preRun = phases.preRun or { };
      postRun = phases.postRun or { };
      plan = workflow.plan or [ ];
    in
    {
      key = workflowId;
      value = {
        modeName = workflow.mode or "custom";
        artifactsRoot = workflow.artifactsRoot or "";
        ephemeralFlag = if workflow.ephemeralEnabled or false then "1" else "0";
        loggingLevelDefault = logging.levelDefault or "";
        loggingOutputDefault = logging.outputDefault or "";
        failFast = if workflow.failFast or false then "true" else "false";
        parallelEnabled = if workflow.parallelEnabled or false then "true" else "false";
        maxWorkers = toString (workflow.maxWorkers or 1);
        writeSummary = if workflow.writeSummary or false then "true" else "false";
        postRunAlways = if workflow.postRunAlways or false then "true" else "false";
        unitClosureSelectedServices = workflow.unitClosureSelectedServices or [ ];
        planTaskIds = map (unit: unit.taskId or "") (
          builtins.filter (unit: (unit.taskId or "") != "") plan
        );
        preTasks = preRun.tasks or [ ];
        postTasks = postRun.tasks or [ ];
      };
    }
  ) workflowIds;

  workflowPhaseTaskEntries = builtins.concatLists (
    map (entry: [
      {
        key = "${entry.key}:preRun";
        value = entry.value.preTasks;
      }
      {
        key = "${entry.key}:postRun";
        value = entry.value.postTasks;
      }
    ]) workflowEntries
  );

  workflowUnitEntries = builtins.concatLists (
    map (
      workflowId:
      let
        workflow = workflows.${workflowId};
      in
      map (
        unit:
        let
          needs = unit.needs or [ ];
          locks = unit.locks or [ ];
          when = unit.when or { };
          whenEnvEquals = when.envEquals or { };
          produces = unit.produces or { };
        in
        {
          key = "workflow-unit:${workflowId}:${unit.name or ""}";
          value = {
            name = unit.name or "";
            taskId = unit.taskId or "";
            needsCount = toString (builtins.length needs);
            needs = needs;
            locks = if locks == [ ] then "" else "${lib.concatStringsSep " " locks} ";
            requiredServices = unit.requiredServices or [ ];
            producesJson = builtins.toJSON {
              artifacts = produces.artifacts or [ ];
              stateKeys = produces.stateKeys or [ ];
            };
            skipIfMissingEnv = unit.skipIfMissingEnv or [ ];
            whenEnvPresent = when.envPresent or [ ];
            whenEnvEquals = map (name: "${name}\t${builtins.toString whenEnvEquals.${name}}") (
              builtins.sort builtins.lessThan (builtins.attrNames whenEnvEquals)
            );
          };
        }
      ) (workflow.plan or [ ])
    ) workflowIds
  );

  workflowFamilyEntries = map (
    family:
    {
      key = family;
      value = {
        modesJoined = lib.concatStringsSep "|" (workflowFamilies.${family}.modes or [ ]);
      };
    }
  ) workflowFamilyIds;

  renderExistsCase =
    keys:
    lib.concatStringsSep "\n" (
      map (key: ''
        ${lib.escapeShellArg key})
          return 0
          ;;
      '') keys
    );

  renderCaseReturn =
    valueExpr: entries:
    lib.concatStringsSep "\n" (
      map (entry: ''
        ${lib.escapeShellArg entry.key})
          printf '%s' ${lib.escapeShellArg (valueExpr entry)}
          return 0
          ;;
      '') entries
    );

  renderCasePrintLines =
    valuesExpr: entries:
    lib.concatStringsSep "\n" (
      map (
        entry:
        let
          values = valuesExpr entry;
        in
        ''
          ${lib.escapeShellArg entry.key})
            ${
              if values == [ ] then
                ":"
              else
                "printf '%s\\n' " + lib.concatStringsSep " " (map lib.escapeShellArg values)
            }
            ;;
        ''
      ) entries
    );
in
''
    ${commonRuntimeShell}
    ${skipPolicy.skipPolicyFunctions}

    task_help_requested() {
      local arg=""

      while [ "$#" -gt 0 ]; do
        arg="$1"
        shift

        case "$arg" in
          --help|-h)
            return 0
            ;;
          --)
            return 1
            ;;
        esac
      done

      return 1
    }

    task_descriptor_exists() {
      case "$1" in
  ${renderExistsCase taskIds}
        *)
          return 1
          ;;
      esac
    }

    workflow_id_exists() {
      case "$1" in
  ${renderExistsCase workflowIds}
        *)
          return 1
          ;;
      esac
    }

    task_print_help() {
      case "$1" in
  ${renderCasePrintLines (entry: entry.value.helpLines) taskEntries}
        *)
          return 1
          ;;
      esac
    }

    task_validate_args() {
      local task_id="$1"
      shift

      ${kernelPackage}/bin/nixfied-kernel task validate-args \
        "$NIXFIED_MODEL_FILE" \
        "$task_id" \
        -- "$@"
    }

    task_runner_type() {
      case "$1" in
  ${renderCaseReturn (entry: entry.value.runnerType) taskEntries}
        *)
          return 1
          ;;
      esac
    }

    task_runner_command() {
      case "$1" in
  ${renderCaseReturn (entry: entry.value.runnerCommand) taskEntries}
        *)
          return 1
          ;;
      esac
    }

    task_runner_package() {
      case "$1" in
  ${renderCaseReturn (entry: entry.value.runnerPackage) taskEntries}
        *)
          return 1
          ;;
      esac
    }

    task_runner_workflow_id() {
      case "$1" in
  ${renderCaseReturn (entry: entry.value.runnerWorkflowId) taskEntries}
        *)
          return 1
          ;;
      esac
    }

    task_required_services() {
      case "$1" in
  ${renderCasePrintLines (entry: entry.value.requiredServices) taskEntries}
        *)
          return 0
          ;;
      esac
    }

    task_closure_selected_services() {
      case "$1" in
  ${renderCasePrintLines (entry: entry.value.closureSelectedServices) taskEntries}
        *)
          return 0
          ;;
      esac
    }

    task_base_closure_selected_services() {
      case "$1" in
  ${renderCasePrintLines (entry: entry.value.baseClosureSelectedServices) taskEntries}
        *)
          return 0
          ;;
      esac
    }

    workflow_unit_closure_selected_services() {
      case "$1" in
  ${renderCasePrintLines (entry: entry.value.unitClosureSelectedServices) workflowEntries}
        *)
          return 0
          ;;
      esac
    }

    workflow_plan_task_ids() {
      case "$1" in
  ${renderCasePrintLines (entry: entry.value.planTaskIds) workflowEntries}
        *)
          return 0
          ;;
      esac
    }

    workflow_phase_tasks() {
      case "$1:$2" in
  ${renderCasePrintLines (entry: entry.value) workflowPhaseTaskEntries}
        *)
          return 0
          ;;
      esac
    }

    workflow_mode_name() {
      case "$1" in
  ${renderCaseReturn (entry: entry.value.modeName) workflowEntries}
        *)
          return 1
          ;;
      esac
    }

    workflow_artifacts_root() {
      case "$1" in
  ${renderCaseReturn (entry: entry.value.artifactsRoot) workflowEntries}
        *)
          return 1
          ;;
      esac
    }

    workflow_ephemeral_flag() {
      case "$1" in
  ${renderCaseReturn (entry: entry.value.ephemeralFlag) workflowEntries}
        *)
          return 1
          ;;
      esac
    }

    workflow_logging_level_default() {
      case "$1" in
  ${renderCaseReturn (entry: entry.value.loggingLevelDefault) workflowEntries}
        *)
          return 1
          ;;
      esac
    }

    workflow_logging_output_default() {
      case "$1" in
  ${renderCaseReturn (entry: entry.value.loggingOutputDefault) workflowEntries}
        *)
          return 1
          ;;
      esac
    }

    workflow_fail_fast() {
      case "$1" in
  ${renderCaseReturn (entry: entry.value.failFast) workflowEntries}
        *)
          return 1
          ;;
      esac
    }

    workflow_parallel_enabled() {
      case "$1" in
  ${renderCaseReturn (entry: entry.value.parallelEnabled) workflowEntries}
        *)
          return 1
          ;;
      esac
    }

    workflow_max_workers() {
      case "$1" in
  ${renderCaseReturn (entry: entry.value.maxWorkers) workflowEntries}
        *)
          return 1
          ;;
      esac
    }

    workflow_write_summary() {
      case "$1" in
  ${renderCaseReturn (entry: entry.value.writeSummary) workflowEntries}
        *)
          return 1
          ;;
      esac
    }

    workflow_post_run_always() {
      case "$1" in
  ${renderCaseReturn (entry: entry.value.postRunAlways) workflowEntries}
        *)
          return 1
          ;;
      esac
    }

    workflow_family_from_id() {
      local workflow_id="$1"
      local remainder=""

      case "$workflow_id" in
        workflow.*.*)
          remainder="''${workflow_id#workflow.}"
          printf '%s' "''${remainder%%.*}"
          ;;
        *)
          printf '%s' ""
          ;;
      esac
    }

    workflow_family_modes_joined() {
      case "$1" in
  ${renderCaseReturn (entry: entry.value.modesJoined) workflowFamilyEntries}
        *)
          printf '%s' ""
          ;;
      esac
    }

    workflow_simple_shorthand_exists_for_family() {
      local workflow_id="$1"
      local candidate="$2"

      workflow_resolve_mode_id "$workflow_id" "$candidate" >/dev/null 2>&1
    }

    workflow_resolve_mode_id() {
      local workflow_id="$1"
      local mode_override="$2"
      local family=""
      local candidate=""
      local expected_modes=""

      if ! workflow_id_exists "$workflow_id"; then
        echo "ERROR: unknown workflow '$workflow_id'" >&2
        return 1
      fi

      if [ -z "$mode_override" ]; then
        printf '%s' "$workflow_id"
        return 0
      fi

      family="$(workflow_family_from_id "$workflow_id")"
      if [ -z "$family" ]; then
        echo "ERROR: workflow '$workflow_id' does not support mode overrides" >&2
        return 1
      fi

      candidate="workflow.$family.$mode_override"
      if workflow_id_exists "$candidate"; then
        printf '%s' "$candidate"
        return 0
      fi

      expected_modes="$(workflow_family_modes_joined "$family")"
      if [ -n "$expected_modes" ]; then
        echo "ERROR: unknown mode '$mode_override' (expected: $expected_modes)" >&2
      else
        echo "ERROR: unknown mode '$mode_override'" >&2
      fi
      return 1
    }

    task_invocation_selected_services() {
      local task_id="$1"
      local resolved_workflow_id="''${2:-}"

      task_base_closure_selected_services "$task_id"
      if [ -n "$resolved_workflow_id" ]; then
        workflow_unit_closure_selected_services "$resolved_workflow_id"
      fi
    }

    task_runtime_plan_shell() {
      case "$1" in
  ${renderCaseReturn (entry: entry.value.runtimePlanShell) taskEntries}
        *)
          return 1
          ;;
      esac
    }

    task_runtime_pass_through_env_names() {
      case "$1" in
  ${renderCasePrintLines (entry: entry.value.passThroughEnvNames) taskEntries}
        *)
          return 0
          ;;
      esac
    }

    task_produces_json() {
      case "$1" in
  ${renderCaseReturn (entry: entry.value.producesJson) taskEntries}
        *)
          return 1
          ;;
      esac
    }

    task_max_attempts() {
      case "$1" in
  ${renderCaseReturn (entry: entry.value.maxAttempts) taskEntries}
        *)
          return 1
          ;;
      esac
    }

    task_retry_backoff_values() {
      case "$1" in
  ${renderCasePrintLines (entry: entry.value.retryBackoffValues) taskEntries}
        *)
          return 0
          ;;
      esac
    }

    task_needs() {
      case "$1" in
  ${renderCasePrintLines (entry: entry.value.needs) taskEntries}
        *)
          return 0
          ;;
      esac
    }

    task_soft_needs() {
      case "$1" in
  ${renderCasePrintLines (entry: entry.value.softNeeds) taskEntries}
        *)
          return 0
          ;;
      esac
    }

    task_hook_count() {
      case "$1" in
  ${renderCaseReturn (entry: entry.value.hookCount) taskEntries}
        *)
          return 1
          ;;
      esac
    }

    task_hook_ids() {
      case "$1:$2" in
  ${renderCasePrintLines (entry: entry.value) taskHookIdEntries}
        *)
          return 0
          ;;
      esac
    }

    task_hook_command() {
      case "$1:$2:$3" in
  ${renderCaseReturn (entry: entry.value.command) taskHookEntries}
        *)
          return 1
          ;;
      esac
    }

    task_hook_runtime_plan_shell() {
      case "$1:$2:$3" in
  ${renderCaseReturn (entry: entry.value.runtimePlanShell) taskHookEntries}
        *)
          return 1
          ;;
      esac
    }

    task_hook_runtime_pass_through_env_names() {
      case "$1:$2:$3" in
  ${renderCasePrintLines (entry: entry.value.passThroughEnvNames) taskHookEntries}
        *)
          return 0
          ;;
      esac
    }

    normalize_run_artifacts_dir() {
      local base_dir="$1"
      local run_id="$2"
      local attempt_id="$3"

      if [ -z "$attempt_id" ]; then
        attempt_id="$run_id"
      fi

      case "$base_dir" in
        */"$attempt_id")
          printf '%s' "$base_dir"
          ;;
        */"$run_id")
          printf '%s/%s' "$base_dir" "$attempt_id"
          ;;
        *)
          printf '%s/%s/%s' "$base_dir" "$run_id" "$attempt_id"
          ;;
      esac
    }

    resolve_run_artifacts_dir() {
      local run_id="$1"
      local workflow_id="$2"
      local caller_root="''${CI_ARTIFACTS_ROOT:-}"
      local caller_dir="''${CI_ARTIFACTS_DIR:-}"
      local attempt_id="''${NIXFIED_ATTEMPT_ID:-''${NIXFIED_ORCHESTRATOR_ATTEMPT_ID:-}}"
      local configured_root=""
      local base_dir=""

      if [ -n "$caller_root" ] && [ -n "$caller_dir" ]; then
        echo "ERROR: CI_ARTIFACTS_ROOT and CI_ARTIFACTS_DIR cannot both be set"
        return 2
      fi

      if [ -n "$workflow_id" ]; then
        configured_root="$(workflow_artifacts_root "$workflow_id")"
      fi

      if [ -n "$caller_root" ]; then
        base_dir="$caller_root"
      elif [ -n "$caller_dir" ]; then
        base_dir="$caller_dir"
      elif [ -n "$configured_root" ]; then
        base_dir="$configured_root"
      elif [ "$REGISTRY_ROOT_EXPLICIT" = "1" ]; then
        base_dir="$REGISTRY_ROOT/artifacts"
      else
        base_dir="$ARTIFACTS_ROOT_DEFAULT"
      fi

      normalize_run_artifacts_dir "$base_dir" "$run_id" "$attempt_id"
    }

    ensure_run_artifacts_dir() {
      local run_id="$1"
      local workflow_id="$2"
      local managed_by_orchestrator="$3"
      local artifacts_dir

      if [ -n "''${CI_ARTIFACTS_DIR:-}" ] && {
        [ "$managed_by_orchestrator" = "1" ] ||
        [ "''${NIXFIED_WORKFLOW_NESTED:-0}" = "1" ] ||
        [ "''${NIXFIED_EXECUTION_EPHEMERAL:-0}" = "1" ]
      }; then
        mkdir -p "$CI_ARTIFACTS_DIR"
        return 0
      fi

      artifacts_dir="$(resolve_run_artifacts_dir "$run_id" "$workflow_id")" || return $?
      export CI_ARTIFACTS_DIR="$artifacts_dir"
      if ! mkdir -p "$CI_ARTIFACTS_DIR"; then
        echo "ERROR: failed to create artifacts directory '$CI_ARTIFACTS_DIR'"
        return 1
      fi
    }

    LOGGING_FILTERED_ARGS=()
    MACHINE_FILTERED_ARGS=()
    MACHINE_RUN_ID_FILE=""
    MACHINE_SUMMARY_FILE=""

    extract_logging_override_args() {
      local parse_options=1
      local arg=""
      local value=""
      local resolved_log_level="''${NIXFIED_CLI_LOG_LEVEL_OVERRIDE:-}"
      local resolved_output_mode="''${NIXFIED_CLI_OUTPUT_MODE_OVERRIDE:-}"

      LOGGING_FILTERED_ARGS=()

      while [ "$#" -gt 0 ]; do
        arg="$1"
        shift

        if [ "$parse_options" -eq 0 ]; then
          LOGGING_FILTERED_ARGS+=("$arg")
          continue
        fi

        case "$arg" in
          --)
            parse_options=0
            LOGGING_FILTERED_ARGS+=("--")
            ;;
          --log-level)
            if [ "$#" -lt 1 ]; then
              echo "ERROR: --log-level requires a value"
              return 2
            fi
            value="$1"
            shift
            if ! valid_log_level "$value"; then
              echo "ERROR: invalid --log-level '$value' (expected: error|warn|info|debug|trace)"
              return 2
            fi
            resolved_log_level="$value"
            ;;
          --log-level=*)
            value="''${arg#--log-level=}"
            if ! valid_log_level "$value"; then
              echo "ERROR: invalid --log-level '$value' (expected: error|warn|info|debug|trace)"
              return 2
            fi
            resolved_log_level="$value"
            ;;
          --output-mode)
            if [ "$#" -lt 1 ]; then
              echo "ERROR: --output-mode requires a value"
              return 2
            fi
            value="$1"
            shift
            if ! valid_output_mode "$value"; then
              echo "ERROR: invalid --output-mode '$value' (expected: stdout|logs|both)"
              return 2
            fi
            resolved_output_mode="$value"
            ;;
          --output-mode=*)
            value="''${arg#--output-mode=}"
            if ! valid_output_mode "$value"; then
              echo "ERROR: invalid --output-mode '$value' (expected: stdout|logs|both)"
              return 2
            fi
            resolved_output_mode="$value"
            ;;
          *)
            LOGGING_FILTERED_ARGS+=("$arg")
            ;;
        esac
      done

      if [ -n "$resolved_log_level" ]; then
        export NIXFIED_CLI_LOG_LEVEL_OVERRIDE="$resolved_log_level"
      else
        unset NIXFIED_CLI_LOG_LEVEL_OVERRIDE || true
      fi

      if [ -n "$resolved_output_mode" ]; then
        export NIXFIED_CLI_OUTPUT_MODE_OVERRIDE="$resolved_output_mode"
      else
        unset NIXFIED_CLI_OUTPUT_MODE_OVERRIDE || true
      fi
    }

    extract_machine_output_args() {
      local parse_options=1
      local arg=""
      local value=""

      MACHINE_FILTERED_ARGS=()
      MACHINE_RUN_ID_FILE="''${NIXFIED_RUN_ID_FILE_OVERRIDE:-}"
      MACHINE_SUMMARY_FILE="''${NIXFIED_SUMMARY_FILE_OVERRIDE:-}"

      while [ "$#" -gt 0 ]; do
        arg="$1"
        shift

        if [ "$parse_options" -eq 0 ]; then
          MACHINE_FILTERED_ARGS+=("$arg")
          continue
        fi

        case "$arg" in
          --)
            parse_options=0
            MACHINE_FILTERED_ARGS+=("--")
            ;;
          --run-id-file)
            if [ "$#" -lt 1 ]; then
              echo "ERROR: --run-id-file requires a value"
              return 2
            fi
            value="$1"
            shift
            if [ -z "$value" ]; then
              echo "ERROR: --run-id-file requires a non-empty value"
              return 2
            fi
            MACHINE_RUN_ID_FILE="$value"
            ;;
          --run-id-file=*)
            value="''${arg#--run-id-file=}"
            if [ -z "$value" ]; then
              echo "ERROR: --run-id-file requires a non-empty value"
              return 2
            fi
            MACHINE_RUN_ID_FILE="$value"
            ;;
          --summary-file)
            if [ "$#" -lt 1 ]; then
              echo "ERROR: --summary-file requires a value"
              return 2
            fi
            value="$1"
            shift
            if [ -z "$value" ]; then
              echo "ERROR: --summary-file requires a non-empty value"
              return 2
            fi
            MACHINE_SUMMARY_FILE="$value"
            ;;
          --summary-file=*)
            value="''${arg#--summary-file=}"
            if [ -z "$value" ]; then
              echo "ERROR: --summary-file requires a non-empty value"
              return 2
            fi
            MACHINE_SUMMARY_FILE="$value"
            ;;
          *)
            MACHINE_FILTERED_ARGS+=("$arg")
            ;;
        esac
      done
    }

    workflow_unit_name() {
      local unit_json="$1"
      case "$unit_json" in
  ${renderCaseReturn (entry: entry.value.name) workflowUnitEntries}
        *)
          return 1
          ;;
      esac
    }

    workflow_unit_task_id() {
      local unit_json="$1"
      case "$unit_json" in
  ${renderCaseReturn (entry: entry.value.taskId) workflowUnitEntries}
        *)
          return 1
          ;;
      esac
    }

    workflow_unit_required_services() {
      local unit_json="$1"
      case "$unit_json" in
  ${renderCasePrintLines (entry: entry.value.requiredServices) workflowUnitEntries}
        *)
          return 0
          ;;
      esac
    }

    workflow_unit_needs_count() {
      local unit_json="$1"
      case "$unit_json" in
  ${renderCaseReturn (entry: entry.value.needsCount) workflowUnitEntries}
        *)
          return 1
          ;;
      esac
    }

    workflow_unit_dependencies() {
      local unit_json="$1"
      case "$unit_json" in
  ${renderCasePrintLines (entry: entry.value.needs) workflowUnitEntries}
        *)
          return 0
          ;;
      esac
    }

    workflow_unit_lock_list() {
      local unit_json="$1"
      case "$unit_json" in
  ${renderCaseReturn (entry: entry.value.locks) workflowUnitEntries}
        *)
          return 0
          ;;
      esac
    }

    workflow_unit_produces_json() {
      local unit_json="$1"
      case "$unit_json" in
  ${renderCaseReturn (entry: entry.value.producesJson) workflowUnitEntries}
        *)
          return 1
          ;;
      esac
    }

    workflow_unit_missing_env_csv() {
      local unit_json="$1"
      local missing=""
      local required_env

      while IFS= read -r required_env; do
        if [ -n "$required_env" ] && [ -z "''${!required_env:-}" ]; then
          if [ -z "$missing" ]; then
            missing="$required_env"
          else
            missing="$missing,$required_env"
          fi
        fi
      done <<< "$(
        case "$unit_json" in
  ${renderCasePrintLines (entry: entry.value.skipIfMissingEnv) workflowUnitEntries}
          *)
            :
            ;;
        esac
      )"

      printf '%s' "$missing"
    }

    workflow_unit_when_matches() {
      local unit_json="$1"
      local required_env
      local env_name
      local expected_value
      local actual_value

      while IFS= read -r required_env; do
        if [ -n "$required_env" ] && [ -z "''${!required_env:-}" ]; then
          return 1
        fi
      done <<< "$(
        case "$unit_json" in
  ${renderCasePrintLines (entry: entry.value.whenEnvPresent) workflowUnitEntries}
          *)
            :
            ;;
        esac
      )"

      while IFS=$'\t' read -r env_name expected_value; do
        if [ -z "$env_name" ]; then
          continue
        fi
        actual_value="''${!env_name:-}"
        if [ "$actual_value" != "$expected_value" ]; then
          return 1
        fi
      done <<< "$(
        case "$unit_json" in
  ${renderCasePrintLines (entry: entry.value.whenEnvEquals) workflowUnitEntries}
          *)
            :
            ;;
        esac
      )"

      return 0
    }
''
