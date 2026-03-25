{
  pkgs,
  model,
  selectionIndex ? null,
  services,
  runtimeHash ? model.identity.evalHash,
  registry,
  projectRoot,
  serviceSetPrograms ? { },
  serviceHookEnv ? { },
}:
let
  lib = pkgs.lib;
  kernelPackage = import ./kernel { inherit pkgs; };
  modelSchemaKind =
    if builtins.isAttrs model && model ? schema && builtins.isAttrs model.schema then
      model.schema.kind or ""
    else
      "";
  resolvedSelectionIndex =
    if selectionIndex != null then
      selectionIndex
    else
      import ../../compiler/compile-selection-index.nix { inherit lib; } {
        tasks = model.tasks or { };
        workflows = model.workflows or { };
        serviceCatalog = model.serviceCatalog or { };
      };

  availableServiceNames = builtins.sort builtins.lessThan (
    lib.unique (
      map (
        serviceId:
        let
          service = services.${serviceId};
        in
        service.name or serviceId
      ) (builtins.attrNames services)
    )
  );
  availableServiceNameSet = builtins.listToAttrs (
    map (serviceName: {
      name = serviceName;
      value = true;
    }) availableServiceNames
  );
  executorResolvedServices = builtins.listToAttrs (
    map (
      serviceId:
      let
        service = model.serviceCatalog.${serviceId};
      in
      {
        name = service.name;
        value = {
          enable = service.enable or false;
        }
        // (service.config or { });
      }
    ) (builtins.sort builtins.lessThan (builtins.attrNames (model.serviceCatalog or { })))
  );
  workflowPhaseServiceSetOperations = builtins.foldl' (
    acc: workflowId:
    let
      workflow = model.workflows.${workflowId};
      phaseEntries = (workflow.preRun.serviceSets or [ ]) ++ (workflow.postRun.serviceSets or [ ]);
      addEntry =
        phaseAcc: entry:
        let
          serviceSetId = entry.serviceSetId or "";
          operation = entry.operation or "";
          existing = phaseAcc.${serviceSetId} or [ ];
        in
        if serviceSetId == "" || operation == "" then
          phaseAcc
        else
          phaseAcc
          // {
            ${serviceSetId} = builtins.sort builtins.lessThan (lib.unique (existing ++ [ operation ]));
          };
    in
    builtins.foldl' addEntry acc phaseEntries
  ) { } (builtins.sort builtins.lessThan (builtins.attrNames (model.workflows or { })));
  synthesizedServiceSetPrograms = builtins.mapAttrs (
    serviceSetId: serviceSet:
    let
      effectiveSelectedServices = builtins.filter (
        serviceName: builtins.hasAttr serviceName availableServiceNameSet
      ) (serviceSet.services.all or [ ]);
      requiredOperations = workflowPhaseServiceSetOperations.${serviceSetId} or [ ];
      serviceSetModel = model // {
        runtime = model.runtime // {
          directories = (model.runtime.directories or { }) // {
            base = serviceSet.state.policy.runtimeBase;
          };
        };
        state = model.state // {
          policy = serviceSet.state.policy;
        };
      };
      serviceSetRuntimeSurfaces = import ../core/mkServiceRuntimeSurfaces.nix {
        inherit pkgs;
        model = serviceSetModel;
        services = services;
        selectedServices = effectiveSelectedServices;
      };
    in
    import ../core/mkServiceSetPrograms.nix {
      inherit
        pkgs
        serviceSet
        ;
      model = serviceSetModel;
      resolvedServices = executorResolvedServices;
      serviceRuntimeSurfaces = serviceSetRuntimeSurfaces;
      operations = requiredOperations;
    }
  ) (model.serviceSets or { });
  effectiveServiceSetPrograms =
    if serviceSetPrograms == { } then synthesizedServiceSetPrograms else serviceSetPrograms;

  serviceSetProgramCases = builtins.concatLists (
    map (
      serviceSetId:
      let
        operations = builtins.sort builtins.lessThan (
          builtins.attrNames (effectiveServiceSetPrograms.${serviceSetId}.programsByOperation or { })
        );
      in
      map (operation: {
        key = "${serviceSetId}:${operation}";
        value = effectiveServiceSetPrograms.${serviceSetId}.programsByOperation.${operation}.program;
      }) operations
    ) (builtins.sort builtins.lessThan (builtins.attrNames effectiveServiceSetPrograms))
  );

  modelFile = pkgs.writeText (
    if modelSchemaKind == "nixfied-execution-manifest" then
      "nixfied-execution-manifest.json"
    else
      "nixfied-model.json"
  ) (builtins.toJSON model);
  shellCommon = import ../core/shell-common.nix { inherit pkgs; };
  registryShell = registry.events.mkShellLib { };
  workflowModesShell = import ./workflow-modes.nix {
    inherit
      pkgs
      model
      ;
    selectionIndex = resolvedSelectionIndex;
  };
  envSandboxShell = import ./env-sandbox.nix {
    inherit
      pkgs
      projectRoot
      model
      services
      serviceHookEnv
      ;
  };
  executorRuntimeShell = import ./executor-runtime.nix {
    inherit
      pkgs
      model
      ;
  };
  runtimeArtifactContracts = import ../contracts/runtime-artifact-contracts.nix { inherit pkgs; };
  validationBundleFile = pkgs.writeText "nixfied-runtime-artifact-contract-bundle.json" (
    builtins.toJSON runtimeArtifactContracts.bundle
  );
  availableServiceNamesFile = pkgs.writeText "nixfied-available-service-names.txt" (
    lib.concatStringsSep "\n" availableServiceNames
    + lib.optionalString (availableServiceNames != [ ]) "\n"
  );
  taskDependencyPlanFile = pkgs.writeText "nixfied-task-dependency-plan.json" (
    builtins.toJSON {
      kind = "nixfied-task-dependency-plan";
      version = 1;
      tasks = builtins.mapAttrs (
        _: task: {
          needs = task.needs or [ ];
          softNeeds = task.softNeeds or [ ];
          requiredServices = task.requirements.services or [ ];
        }
      ) (model.tasks or { });
    }
  );
  workflowSummaryPlanFile = pkgs.writeText "nixfied-workflow-summary-plan.json" (
    builtins.toJSON {
      kind = "nixfied-workflow-summary-plan";
      version = 1;
      taskRunnerTypes = builtins.mapAttrs (
        _: task: task.runner.type or "shell"
      ) (model.tasks or { });
    }
  );
in
pkgs.writeShellScriptBin "nixfied-executor" ''
      set -euo pipefail
      ${shellCommon}
      export NIXFIED_EXECUTOR_BIN="$0"
      export NIXFIED_EXECUTOR_SELF="$0"

      MODEL_FILE=${pkgs.lib.escapeShellArg (builtins.toString modelFile)}
      export NIXFIED_MODEL_FILE="$MODEL_FILE"
      PROJECT_ROOT=${pkgs.lib.escapeShellArg (builtins.toString projectRoot)}
      REGISTRY_ROOT_DEFAULT=${pkgs.lib.escapeShellArg model.state.policy.registryRoot}
      ARTIFACTS_ROOT_DEFAULT=${pkgs.lib.escapeShellArg model.state.policy.artifactsRoot}
      if [ -n "''${REGISTRY_ROOT+x}" ]; then
        REGISTRY_ROOT_EXPLICIT=1
      else
        REGISTRY_ROOT_EXPLICIT=0
      fi
      REGISTRY_ROOT="''${REGISTRY_ROOT:-$REGISTRY_ROOT_DEFAULT}"

      ${registryShell}
      ${workflowModesShell}
      ${envSandboxShell}
      ${executorRuntimeShell}

      sha256_text() {
        printf '%s' "$1" | ${pkgs.coreutils}/bin/sha256sum | ${pkgs.gawk}/bin/awk '{print $1}'
      }

      write_name_value_tsv_file() {
        local target_file="$1"
        local value_name=""

        : > "$target_file" || return 1
        while IFS= read -r value_name; do
          [ -n "$value_name" ] || continue
          if [ -z "''${!value_name+x}" ]; then
            continue
          fi
          printf '%s\t%s\n' "$value_name" "''${!value_name}" >> "$target_file" || return 1
        done
      }

      kernel_run_id_envelope() {
        local run_kind="$1"
        local workflow_id="$2"
        local task_id="$3"
        local slot_value="$4"
        local env_value="$5"
        local pass_through_env_file="$6"
        shift 6

        ${kernelPackage}/bin/nixfied-kernel run-id envelope \
          ${pkgs.lib.escapeShellArg "${model.identity.evalHash}"} \
          ${pkgs.lib.escapeShellArg "${runtimeHash}"} \
          "$run_kind" \
          "$workflow_id" \
          "$task_id" \
          "$slot_value" \
          "$env_value" \
          "$pass_through_env_file" \
          -- "$@"
      }

      kernel_event_detail() {
        ${kernelPackage}/bin/nixfied-kernel event-detail render "$@"
      }

      selected_services_csv_from_lines() {
        local service_name=""
        local services_csv=""

        services_csv="$(
          while IFS= read -r service_name; do
            if [ -n "$service_name" ]; then
              printf '%s\n' "$service_name"
            fi
          done | ${pkgs.coreutils}/bin/sort -u | ${pkgs.coreutils}/bin/paste -sd, -
        )"

        printf '%s' "$services_csv"
      }

      workflow_phase_service_set_program() {
        local service_set_id="$1"
        local operation="$2"
        case "$service_set_id:$operation" in
    ${builtins.concatStringsSep "\n" (
      map (entry: ''
        ${pkgs.lib.escapeShellArg entry.key})
          printf '%s' ${pkgs.lib.escapeShellArg entry.value}
          return 0
          ;;
      '') serviceSetProgramCases
    )}
          *)
            return 1
            ;;
        esac
      }

      run_id_pass_through_env_file() {
        local run_kind="$1"
        local workflow_id="$2"
        local task_id="$3"
        local env_file=""
        local env_names_file=""
        local env_name=""
        local dep_task_id=""
        local phase=""
        local hook_id=""
        local runner_type=""
        local nested_workflow_id=""
        local unit_json=""
        local unit_task_id=""
        local -A seen_tasks
        local -A seen_workflows

        collect_task_env_names() {
          local current_task_id="$1"

          if [ -z "$current_task_id" ] || [ -n "''${seen_tasks[$current_task_id]:-}" ]; then
            return 0
          fi
          seen_tasks[$current_task_id]=1

          task_runtime_pass_through_env_names "$current_task_id"

          for phase in pre post; do
            while IFS= read -r hook_id; do
              [ -n "$hook_id" ] || continue
              task_hook_runtime_pass_through_env_names "$current_task_id" "$phase" "$hook_id"
            done < <(task_hook_ids "$current_task_id" "$phase" 2>/dev/null || true)
          done

          while IFS= read -r dep_task_id; do
            [ -n "$dep_task_id" ] || continue
            collect_task_env_names "$dep_task_id"
          done < <(task_needs "$current_task_id" 2>/dev/null || true)

          while IFS= read -r dep_task_id; do
            [ -n "$dep_task_id" ] || continue
            collect_task_env_names "$dep_task_id"
          done < <(task_soft_needs "$current_task_id" 2>/dev/null || true)

          runner_type="$(task_runner_type "$current_task_id")"
          if [ "$runner_type" = "workflowRef" ]; then
            nested_workflow_id="$(task_runner_workflow_id "$current_task_id")"
            if [ -n "$nested_workflow_id" ]; then
              collect_workflow_env_names "$nested_workflow_id"
            fi
          fi
        }

        collect_workflow_env_names() {
          local current_workflow_id="$1"

          if [ -z "$current_workflow_id" ] || [ -n "''${seen_workflows[$current_workflow_id]:-}" ]; then
            return 0
          fi
          seen_workflows[$current_workflow_id]=1

          while IFS= read -r dep_task_id; do
            [ -n "$dep_task_id" ] || continue
            collect_task_env_names "$dep_task_id"
          done < <(workflow_phase_tasks "$current_workflow_id" preRun 2>/dev/null || true)

          while IFS= read -r unit_json; do
            [ -n "$unit_json" ] || continue
            unit_task_id="$(workflow_unit_task_id "$unit_json")"
            if [ -n "$unit_task_id" ] && [ "$unit_task_id" != "null" ]; then
              collect_task_env_names "$unit_task_id"
            fi
          done < <(workflow_plan_records "$current_workflow_id" 2>/dev/null || true)

          while IFS= read -r dep_task_id; do
            [ -n "$dep_task_id" ] || continue
            collect_task_env_names "$dep_task_id"
          done < <(workflow_phase_tasks "$current_workflow_id" postRun 2>/dev/null || true)
        }

        env_names_file="$(mktemp "''${TMPDIR:-/tmp}/nixfied-run-id-env-names.XXXXXX")" || return 1

        case "$run_kind" in
          task)
            collect_task_env_names "$task_id"
            ;;
          workflow)
            collect_workflow_env_names "$workflow_id"
            ;;
        esac | ${pkgs.coreutils}/bin/sort -u > "$env_names_file"

        env_file="$(mktemp "''${TMPDIR:-/tmp}/nixfied-run-id-env.XXXXXX")" || return 1
        if ! write_name_value_tsv_file "$env_file" < "$env_names_file"; then
          rm -f "$env_file" "$env_names_file"
          return 1
        fi
        rm -f "$env_names_file"
        printf '%s' "$env_file"
      }

      event_detail_mode_json() {
        local mode="$1"
        kernel_event_detail slotLifecycle --mode "$mode"
      }

      event_detail_mode_suffix_json() {
        local mode="$1"
        local suffix_reason="$2"
        kernel_event_detail slotLifecycle --mode "$mode" --suffix-reason "$suffix_reason"
      }

      event_detail_exit_code_json() {
        local exit_code="$1"
        kernel_event_detail slotLifecycle --exit-code "$exit_code"
      }

      event_detail_produces_json() {
        local produces_json="$1"
        kernel_event_detail slotLifecycle --produces-json "$produces_json"
      }

      event_detail_reason_json() {
        local reason="$1"
        kernel_event_detail serviceLifecycle --reason "$reason"
      }

      event_detail_reason_key_value_json() {
        local reason="$1"
        local extra_key="$2"
        local extra_value="$3"
        case "$extra_key" in
          dependency)
            kernel_event_detail serviceLifecycle --reason "$reason" --dependency "$extra_value"
            ;;
          serviceName)
            kernel_event_detail serviceLifecycle --reason "$reason" --service-name "$extra_value"
            ;;
          missing)
            kernel_event_detail serviceLifecycle --reason "$reason" --missing "$extra_value"
            ;;
          *)
            echo "ERROR: unsupported event detail field '$extra_key'" >&2
            return 1
            ;;
        esac
      }

      event_detail_reason_exit_code_json() {
        local reason="$1"
        local exit_code="$2"
        kernel_event_detail serviceLifecycle --reason "$reason" --exit-code "$exit_code"
      }

      workflow_phase_service_set_detail_json() {
        local phase="$1"
        local service_set_id="$2"
        local service_set_name="$3"
        local operation="$4"

        kernel_event_detail \
          serviceLifecycle \
          --event-type "$phase" \
          --service "$service_set_name" \
          --command-name "$operation" \
          --owner-scope "$service_set_id"
      }

      workflow_phase_service_set_failure_json() {
        local phase="$1"
        local service_set_id="$2"
        local service_set_name="$3"
        local operation="$4"
        local exit_code="$5"

        kernel_event_detail \
          serviceLifecycle \
          --event-type "$phase" \
          --service "$service_set_name" \
          --command-name "$operation" \
          --owner-scope "$service_set_id" \
          --exit-code "$exit_code"
      }

      workflow_mode_override_from_args() {
        local workflow_id="$1"
        shift

        local parse_options=1
        local arg=""
        local shorthand_mode=""
        local mode_override=""

        while [ "$#" -gt 0 ]; do
          arg="$1"
          shift

          if [ "$parse_options" -eq 0 ]; then
            continue
          fi

          case "$arg" in
            --mode)
              if [ "$#" -lt 1 ]; then
                break
              fi
              mode_override="$1"
              shift
              ;;
            --mode=*)
              mode_override="''${arg#--mode=}"
              ;;
            --summary)
              ;;
            --)
              parse_options=0
              ;;
            --*)
              shorthand_mode="''${arg#--}"
              if workflow_simple_shorthand_exists_for_family "$workflow_id" "$shorthand_mode"; then
                mode_override="$shorthand_mode"
              fi
              ;;
          esac
        done

        printf '%s' "$mode_override"
      }

      task_selected_services_csv() {
        local task_id="$1"
        shift

        local runner_type=""
        local workflow_id=""
        local mode_override=""
        local resolved_workflow_id=""

        runner_type="$(task_runner_type "$task_id")"
        if [ "$runner_type" = "workflowRef" ]; then
          workflow_id="$(task_runner_workflow_id "$task_id")"
          if [ -n "$workflow_id" ]; then
            mode_override="$(workflow_mode_override_from_args "$workflow_id" "$@")"
            resolved_workflow_id="$(resolve_workflow_mode "$workflow_id" "$mode_override")" || return $?
          fi
        fi

        task_invocation_selected_services "$task_id" "$resolved_workflow_id" | selected_services_csv_from_lines
      }

      workflow_unit_selected_services_csv() {
        local unit_json="$1"
        local task_id=""
        local runner_type=""
        local workflow_id=""

        task_id="$(workflow_unit_task_id "$unit_json")"

        {
          workflow_unit_required_services "$unit_json"

          if [ -n "$task_id" ]; then
            runner_type="$(task_runner_type "$task_id")"
            if [ "$runner_type" = "workflowRef" ]; then
              workflow_id="$(task_runner_workflow_id "$task_id")"
            else
              workflow_id=""
            fi
            task_invocation_selected_services "$task_id" "$workflow_id"
          fi
        } | selected_services_csv_from_lines
      }

      RUN_SUFFIX_REASON=""
      LAST_WORKFLOW_SUMMARY_FILE=""
      LAST_WORKFLOW_ATTEMPT_ID=""
      LAST_WORKFLOW_SUMMARY_PASSED_COUNT="0"
      LAST_WORKFLOW_SUMMARY_FAILED_COUNT="0"
      LAST_WORKFLOW_SUMMARY_SKIPPED_COUNT="0"
      LAST_WORKFLOW_SUMMARY_CANCELED_COUNT="0"

      compute_attempt_id() {
        local attempt_dir
        local attempt_name

        attempt_dir="$(mktemp -d "''${TMPDIR:-/tmp}/nixfied-attempt.XXXXXX")" || return 1
        attempt_name="$(basename "$attempt_dir")"
        rmdir "$attempt_dir"
        printf '%s' "attempt-''${attempt_name#nixfied-attempt.}"
      }

      compute_run_id() {
        local run_kind="$1"
        local workflow_id="$2"
        local task_id="$3"
        shift 3

        local slot_var=${pkgs.lib.escapeShellArg model.runtime.slot.var}
        local env_var=${pkgs.lib.escapeShellArg model.runtime.env.var}
        local slot_default=${toString model.runtime.slot.default}
        local env_default=${pkgs.lib.escapeShellArg model.runtime.env.default}

        local slot_value
        local env_value
        local pass_through_env_file
        local run_input
        local run_base
        local run_id
        local attempt_id=""

        slot_value="''${!slot_var:-$slot_default}"
        env_value="''${!env_var:-$env_default}"
        pass_through_env_file="$(run_id_pass_through_env_file "$run_kind" "$workflow_id" "$task_id")" || return 1

        run_input="$(kernel_run_id_envelope "$run_kind" "$workflow_id" "$task_id" "$slot_value" "$env_value" "$pass_through_env_file" "$@")" || {
          rm -f "$pass_through_env_file"
          return 1
        }
        rm -f "$pass_through_env_file"
        run_base="$(sha256_text "$run_input")"
        run_id="run-''${run_base:0:24}"
        RUN_SUFFIX_REASON=""

        mkdir -p "$REGISTRY_ROOT/active" "$REGISTRY_ROOT/counters"

        if [ -e "$REGISTRY_ROOT/active/$run_id" ]; then
          local lock_file="$REGISTRY_ROOT/counters/$run_base.lock"
          local counter_file="$REGISTRY_ROOT/counters/$run_base"
          local counter="0"
          local lock_fd

          lock_fd="$(registry_lock_acquire "$lock_file" "executor-run-counter:$run_base" 30)" || return 1
          if [ -f "$counter_file" ]; then
            counter="$(cat "$counter_file")"
          fi
          counter="$(( counter + 1 ))"
          printf '%s' "$counter" > "$counter_file"
          registry_lock_release "$lock_fd" "$lock_file"

          run_id="$run_id-$(printf 'c%03d' "$counter")"
          RUN_SUFFIX_REASON="active-collision"
        fi

        printf '%s' "$run_id"
      }

      activate_run() {
        local run_id="$1"
        mkdir -p "$REGISTRY_ROOT/active"
        : > "$REGISTRY_ROOT/active/$run_id"
      }

      deactivate_run() {
        local run_id="$1"
        rm -f "$REGISTRY_ROOT/active/$run_id"
      }

      append_event() {
        local run_id="$1"
        local workflow_id="$2"
        local task_id="$3"
        local state="$4"
        local detail_json="$5"
        local detail_reason="''${6:-}"
        local detail_exit_code="''${7:-}"
        local attempt_id="''${NIXFIED_ATTEMPT_ID:-}"

        registry_append_event "$REGISTRY_ROOT" "$run_id" "$attempt_id" "$workflow_id" "$task_id" "$state" "$detail_json" "$detail_reason" "$detail_exit_code"
      }

      task_has_hooks() {
        local hook_count

        hook_count="$(task_hook_count "$1")"
        if [ "$hook_count" -gt 0 ]; then
          return 0
        fi
        return 1
      }

      run_task_hooks() {
        local task_id="$1"
        local phase="$2"
        shift 2

        local hook_id
        local hook_command
        local hook_runtime_plan_shell
        local hook_exit_code

        while IFS= read -r hook_id; do
          if [ -z "$hook_id" ]; then
            continue
          fi

          echo "INFO: hook $phase $hook_id start"
          hook_command="$(task_hook_command "$task_id" "$phase" "$hook_id")" || return 3
          hook_runtime_plan_shell="$(task_hook_runtime_plan_shell "$task_id" "$phase" "$hook_id")" || return 3

          run_in_sandbox_runtime "$hook_runtime_plan_shell" "$hook_command" "$@"
          hook_exit_code="$?"
          if [ "$hook_exit_code" -ne 0 ]; then
            echo "ERROR: hook $phase $hook_id failed exitCode=$hook_exit_code"
            return "$hook_exit_code"
          fi

          echo "OK: hook $phase $hook_id done"
        done < <(task_hook_ids "$task_id" "$phase")

        return 0
      }

      task_pass_detail_json() {
        local task_id="$1"
        if ! task_descriptor_exists "$task_id"; then
          printf '%s' '{}'
          return 0
        fi
        task_produces_json "$task_id"
      }

      task_retry_backoff_for_attempt() {
        local task_id="$1"
        local retry_index="$2"
        local backoff_value=""
        local -a backoff_values=()

        while IFS= read -r backoff_value; do
          [ -n "$backoff_value" ] || continue
          backoff_values+=("$backoff_value")
        done < <(task_retry_backoff_values "$task_id")

        if [ "''${#backoff_values[@]}" -eq 0 ]; then
          printf '%s' "0"
        elif [ "$retry_index" -lt "''${#backoff_values[@]}" ]; then
          printf '%s' "''${backoff_values[$retry_index]}"
        else
          printf '%s' "''${backoff_values[$(( ''${#backoff_values[@]} - 1 ))]}"
        fi
      }

      execute_task_once() {
        local task_id="$1"
        shift

        local runner_type
        local command
        local nested_workflow
        local package_path
        local runtime_plan_shell
        local exit_code
        local main_exit_code
        local post_exit_code

        runner_type="$(task_runner_type "$task_id")"
        if [ "$runner_type" != "shell" ] && task_has_hooks "$task_id"; then
          echo "ERROR: task '$task_id' defines runtime hooks but runner type '$runner_type' is unsupported"
          return 3
        fi
        runtime_plan_shell="$(task_runtime_plan_shell "$task_id")" || return 3

        set +e
        case "$runner_type" in
          shell)
            if run_task_hooks "$task_id" "pre" "$@"; then
              command="$(task_runner_command "$task_id")"
              run_in_sandbox_runtime "$runtime_plan_shell" "$command" "$@"
              main_exit_code="$?"

              if run_task_hooks "$task_id" "post" "$@"; then
                post_exit_code=0
              else
                post_exit_code="$?"
              fi

              if [ "$post_exit_code" -ne 0 ]; then
                if [ "$main_exit_code" -ne 0 ]; then
                  echo "ERROR: task '$task_id' main exitCode=$main_exit_code and post hook failed exitCode=$post_exit_code"
                fi
                exit_code="$post_exit_code"
              else
                exit_code="$main_exit_code"
              fi
            else
              exit_code="$?"
            fi
            ;;
          workflowRef)
            nested_workflow="$(task_runner_workflow_id "$task_id")"
            if [ -z "$nested_workflow" ]; then
              echo "ERROR: task '$task_id' runner.workflowId is empty"
              exit_code=3
            else
              if [ -n "''${NIXFIED_PARENT_WORKFLOW_ID:-}" ]; then
                NIXFIED_WORKFLOW_NESTED=1 run_workflow "$nested_workflow" "$@"
              else
                run_workflow "$nested_workflow" "$@"
              fi
              exit_code="$?"
            fi
            ;;
          derivation)
            package_path="$(task_runner_package "$task_id")"
            command="$(task_runner_command "$task_id")"
            if [ -z "$package_path" ]; then
              echo "ERROR: task '$task_id' derivation runner requires runner.package"
              exit_code=3
            elif [ -z "$command" ]; then
              echo "ERROR: task '$task_id' derivation runner requires runner.command"
              exit_code=3
            else
              run_in_sandbox_runtime "$runtime_plan_shell" "$command" "$@"
              exit_code="$?"
            fi
            ;;
          *)
            echo "ERROR: unsupported runner type '$runner_type' for task '$task_id'"
            exit_code=3
            ;;
        esac
        set -e

        return "$exit_code"
      }

      execute_task_body() {
        local task_id="$1"
        shift

        local max_attempts
        local attempt=1
        local exit_code=0
        local retry_index
        local backoff_sec

        if ! task_descriptor_exists "$task_id"; then
          echo "ERROR: unknown task '$task_id'"
          return "$NIXFIED_EXIT_USAGE"
        fi

        max_attempts="$(task_max_attempts "$task_id")"

        while [ "$attempt" -le "$max_attempts" ]; do
          if execute_task_once "$task_id" "$@"; then
            return 0
          else
            exit_code="$?"
          fi

          if [ "$attempt" -ge "$max_attempts" ]; then
            return "$exit_code"
          fi

          retry_index=$((attempt - 1))
          backoff_sec="$(task_retry_backoff_for_attempt "$task_id" "$retry_index")"
          if ! [[ "$backoff_sec" =~ ^[0-9]+$ ]]; then
            backoff_sec=0
          fi
          echo "WARN: task '$task_id' retrying attempt=$((attempt + 1))/$max_attempts after=''${backoff_sec}s exitCode=$exit_code"
          if [ "$backoff_sec" -gt 0 ]; then
            sleep "$backoff_sec"
          fi
          attempt=$((attempt + 1))
        done

        return "$exit_code"
      }

      execute_task() {
        local run_id="$1"
        local workflow_id="$2"
        local task_id="$3"
        shift 3

        local detail_json
        local exit_code
        local effective_workflow_id
        local selected_services_csv=""

        detail_json="$(event_detail_mode_json "task")"
        append_event "$run_id" "$workflow_id" "$task_id" "queued" "$detail_json"
        append_event "$run_id" "$workflow_id" "$task_id" "running" "$detail_json"

        if [ -z "$workflow_id" ]; then
          effective_workflow_id="task-root"
        else
          effective_workflow_id="$workflow_id"
        fi
        echo "INFO: task context runId=$run_id workflowId=$effective_workflow_id taskId=$task_id"

        if [ -n "''${NIXFIED_SELECTED_SERVICES_CSV_OVERRIDE+x}" ]; then
          selected_services_csv="''${NIXFIED_SELECTED_SERVICES_CSV_OVERRIDE}"
        else
          selected_services_csv="$(task_selected_services_csv "$task_id" "$@")"
        fi

        if [ -n "$workflow_id" ]; then
          if NIXFIED_TASK_ID="$task_id" NIXFIED_PARENT_WORKFLOW_ID="$workflow_id" NIXFIED_SELECTED_SERVICES_CSV="$selected_services_csv" execute_task_body "$task_id" "$@"; then
            exit_code=0
          else
            exit_code="$?"
          fi
        elif NIXFIED_TASK_ID="$task_id" NIXFIED_SELECTED_SERVICES_CSV="$selected_services_csv" execute_task_body "$task_id" "$@"; then
          exit_code=0
        else
          exit_code="$?"
        fi

        if [ "$exit_code" -eq 0 ]; then
          detail_json="$(event_detail_produces_json "$(task_pass_detail_json "$task_id")")"
          append_event "$run_id" "$workflow_id" "$task_id" "passed" "$detail_json"
        else
          detail_json="$(event_detail_exit_code_json "$exit_code")"
          append_event "$run_id" "$workflow_id" "$task_id" "failed" "$detail_json" "" "$exit_code"
          return "$exit_code"
        fi
      }

      task_first_skipped_required_service() {
        local task_id="$1"
        local service_name=""

        while IFS= read -r service_name; do
          if [ -n "$service_name" ] && is_service_skipped "$service_name"; then
            printf '%s' "$service_name"
            return 0
          fi
        done < <(task_required_services "$task_id")

        return 1
      }

      workflow_unit_first_skipped_required_service() {
        local unit_json="$1"
        local service_name=""

        while IFS= read -r service_name; do
          if [ -n "$service_name" ] && is_service_skipped "$service_name"; then
            printf '%s' "$service_name"
            return 0
          fi
        done < <(workflow_unit_required_services "$unit_json")

        return 1
      }

      write_skipped_services_file() {
        local target_file="$1"
        local service_name=""

        : > "$target_file" || return 1
        while IFS= read -r service_name; do
          [ -n "$service_name" ] || continue
          if is_service_skipped "$service_name"; then
            printf '%s\n' "$service_name" >> "$target_file" || return 1
          fi
        done < ${pkgs.lib.escapeShellArg (builtins.toString availableServiceNamesFile)}
      }

      task_execution_plan() {
        local task_id="$1"
        local skipped_services_file="$2"
        local plan_target="$3"
        local export_target="$4"

        ${kernelPackage}/bin/nixfied-kernel task execution-order \
          ${pkgs.lib.escapeShellArg (builtins.toString taskDependencyPlanFile)} \
          "$skipped_services_file" \
          "$task_id" \
          "$plan_target" \
          "$export_target" \
          >/dev/null
      }

      run_task() {
        if [ "$#" -lt 1 ]; then
          echo "ERROR: usage: run-task <task-id> [-- ...]"
          return "$NIXFIED_EXIT_USAGE"
        fi

        local task_id="$1"
        shift

        local run_id
        local attempt_id=""
        local detail_json
        local status
        local runner_type
        local managed_by_orchestrator=0
        local NIXFIED_WORKFLOW_CONTEXT="0"
        local -a filtered_args
        filtered_args=()

        extract_logging_override_args "$@" || return $?
        filtered_args=("''${LOGGING_FILTERED_ARGS[@]}")
        extract_machine_output_args "''${filtered_args[@]}" || return $?
        filtered_args=("''${MACHINE_FILTERED_ARGS[@]}")

        if ! task_descriptor_exists "$task_id"; then
          echo "ERROR: unknown task '$task_id'"
          return "$NIXFIED_EXIT_USAGE"
        fi
        if task_help_requested "''${filtered_args[@]}"; then
          if ! task_print_help "$task_id"; then
            echo "ERROR: unknown task '$task_id'"
            return "$NIXFIED_EXIT_USAGE"
          fi
          return 0
        fi
        runner_type="$(task_runner_type "$task_id")"

        if [ "$runner_type" != "workflowRef" ] && [ -n "$MACHINE_SUMMARY_FILE" ]; then
          echo "ERROR: --summary-file is only supported for workflow runs"
          return "$NIXFIED_EXIT_USAGE"
        fi

        if [ "''${NIXFIED_ORCHESTRATOR_MANAGED:-0}" = "1" ] && [ -n "''${NIXFIED_ORCHESTRATOR_RUN_ID:-}" ]; then
          run_id="$NIXFIED_ORCHESTRATOR_RUN_ID"
          attempt_id="''${NIXFIED_ORCHESTRATOR_ATTEMPT_ID:-}"
          RUN_SUFFIX_REASON="''${NIXFIED_ORCHESTRATOR_RUN_SUFFIX_REASON:-orchestrator}"
          managed_by_orchestrator=1
        else
          run_id="$(compute_run_id "task" "" "$task_id" "''${filtered_args[@]}")"
          attempt_id="$(compute_attempt_id)"
          activate_run "$run_id"
          trap "deactivate_run '$run_id'" EXIT
        fi
        if [ -z "$attempt_id" ]; then
          attempt_id="$(compute_attempt_id)"
        fi

        ensure_run_artifacts_dir "$run_id" "" "$managed_by_orchestrator" || return $?
        export NIXFIED_RUN_ID="$run_id"
        export NIXFIED_ATTEMPT_ID="$attempt_id"

        if [ "$runner_type" != "workflowRef" ] && [ -n "$MACHINE_RUN_ID_FILE" ]; then
          write_text_file_atomic "$MACHINE_RUN_ID_FILE" "$run_id" || return $?
        fi

        detail_json="$(event_detail_mode_suffix_json "task" "$RUN_SUFFIX_REASON")"
        append_event "$run_id" "" "$task_id" "queued" "$detail_json"

        run_task_with_deps() {
          local root_task="$1"
          shift
          local skipped_services_file=""
          local plan_file=""
          local export_file=""
          local current_task=""
          local action=""
          local soft_parent=""
          local skip_service=""
          local missing_soft_parent=""
          local missing_soft_task=""
          local rc=0
          local skip_detail_json
          skipped_services_file="$(mktemp "''${TMPDIR:-/tmp}/nixfied-task-skipped-services.XXXXXX")" || {
            echo "ERROR: failed to create task skipped-services temp file"
            return 1
          }
          plan_file="$(mktemp "''${TMPDIR:-/tmp}/nixfied-task-execution-order.XXXXXX")" || {
            rm -f "$skipped_services_file"
            echo "ERROR: failed to create task execution-order temp file"
            return 1
          }
          export_file="$(mktemp "''${TMPDIR:-/tmp}/nixfied-task-execution-exports.XXXXXX")" || {
            rm -f "$skipped_services_file" "$plan_file"
            echo "ERROR: failed to create task execution-order export temp file"
            return 1
          }

          if ! write_skipped_services_file "$skipped_services_file"; then
            rm -f "$skipped_services_file" "$plan_file" "$export_file"
            return 1
          fi

          if ! task_execution_plan "$root_task" "$skipped_services_file" "$plan_file" "$export_file"; then
            rm -f "$skipped_services_file" "$plan_file" "$export_file"
            return 1
          fi

          if ! . "$export_file"; then
            rm -f "$skipped_services_file" "$plan_file" "$export_file"
            echo "ERROR: failed to load task execution-order exports"
            return 1
          fi

          if [ -n "''${TASK_EXECUTION_PLAN_SOFT_MISSING_LINES:-}" ]; then
            while IFS=$'\t' read -r missing_soft_parent missing_soft_task; do
              if [ -z "$missing_soft_parent" ] || [ -z "$missing_soft_task" ]; then
                continue
              fi
              echo "WARN: task '$missing_soft_parent' soft dependency '$missing_soft_task' is not defined"
            done <<EOF
''${TASK_EXECUTION_PLAN_SOFT_MISSING_LINES}
EOF
          fi

          while IFS=$'\x1f' read -r current_task action soft_parent skip_service; do
            [ -n "$current_task" ] || continue

            case "$action" in
              service-skipped)
                echo "SKIP: task '$current_task' is skipped because service '$skip_service' has a skip flag enabled"
                skip_detail_json="$(event_detail_reason_key_value_json "service-skipped" "serviceName" "$skip_service")"
                append_event "$run_id" "" "$current_task" "canceled" "$skip_detail_json" "service-skipped"
                if [ -n "$soft_parent" ]; then
                  echo "WARN: task '$soft_parent' soft dependency '$current_task' failed exitCode=3"
                  continue
                fi
                if [ "$current_task" = "$root_task" ]; then
                  rc=0
                else
                  rc=3
                fi
                break
                ;;
              execute)
                if [ "$current_task" = "$task_id" ] && [ "$runner_type" = "workflowRef" ]; then
                  export NIXFIED_RUN_ID_FILE_OVERRIDE="$MACHINE_RUN_ID_FILE"
                  export NIXFIED_SUMMARY_FILE_OVERRIDE="$MACHINE_SUMMARY_FILE"
                fi

                if execute_task "$run_id" "" "$current_task" "$@"; then
                  rc=0
                else
                  rc="$?"
                fi

                if [ "$current_task" = "$task_id" ] && [ "$runner_type" = "workflowRef" ]; then
                  unset NIXFIED_RUN_ID_FILE_OVERRIDE || true
                  unset NIXFIED_SUMMARY_FILE_OVERRIDE || true
                fi

                if [ "$rc" -ne 0 ]; then
                  if [ -n "$soft_parent" ]; then
                    echo "WARN: task '$soft_parent' soft dependency '$current_task' failed exitCode=$rc"
                    continue
                  fi
                  break
                fi
                ;;
              *)
                echo "ERROR: unsupported task execution-order action '$action'"
                rc=1
                break
                ;;
            esac
          done < "$plan_file"

          rm -f "$skipped_services_file" "$plan_file" "$export_file"
          return "$rc"
        }

        set +e
        run_task_with_deps "$task_id" "''${filtered_args[@]}"
        status="$?"
        set -e

        if [ "$managed_by_orchestrator" -eq 0 ]; then
          trap - EXIT
          deactivate_run "$run_id"
        fi

        return "$status"
      }

      resolve_workflow_mode() {
        local workflow_id="$1"
        local mode_override="$2"

        workflow_resolve_mode_id "$workflow_id" "$mode_override"
      }

      resolve_effective_max_workers() {
        local workflow_id="$1"
        local workflow_max_workers
        local effective_workers
        local override_name=""
        local override_value=""

        workflow_max_workers="$(workflow_max_workers "$workflow_id")"
        if ! [[ "$workflow_max_workers" =~ ^[0-9]+$ ]] || [ "$workflow_max_workers" -lt 1 ]; then
          workflow_max_workers=1
        fi
        effective_workers="$workflow_max_workers"

        if [ -n "''${NIXFIED_CI_MAX_WORKERS:-}" ]; then
          override_name="NIXFIED_CI_MAX_WORKERS"
          override_value="$NIXFIED_CI_MAX_WORKERS"
        elif [ -n "''${CI_MAX_WORKERS:-}" ]; then
          override_name="CI_MAX_WORKERS"
          override_value="$CI_MAX_WORKERS"
        fi

        if [ -n "$override_name" ]; then
          if [[ "$override_value" =~ ^[0-9]+$ ]] && [ "$override_value" -ge 1 ]; then
            if [ "$override_value" -lt "$effective_workers" ]; then
              effective_workers="$override_value"
            fi
          else
            echo "ERROR: $override_name must be an integer >= 1 (got '$override_value')" >&2
            return "$NIXFIED_EXIT_USAGE"
          fi
        fi

        printf '%s' "$effective_workers"
      }

      parallel_worker_cap_override_error() {
        local override_name=""
        local override_value=""

        if [ -n "''${NIXFIED_CI_MAX_WORKERS:-}" ]; then
          override_name="NIXFIED_CI_MAX_WORKERS"
          override_value="$NIXFIED_CI_MAX_WORKERS"
        elif [ -n "''${CI_MAX_WORKERS:-}" ]; then
          override_name="CI_MAX_WORKERS"
          override_value="$CI_MAX_WORKERS"
        fi

        if [ -z "$override_name" ]; then
          return 1
        fi

        if [[ "$override_value" =~ ^[0-9]+$ ]] && [ "$override_value" -ge 1 ]; then
          return 1
        fi

        printf "ERROR: %s must be an integer >= 1 (got '%s')" "$override_name" "$override_value"
        return 0
      }

      resolve_parallel_mode() {
        local workflow_id="$1"
        local configured_parallel
        local env_override
        local run_parallel=0

        configured_parallel="$(workflow_parallel_enabled "$workflow_id")"
        if [ "$configured_parallel" = "true" ]; then
          run_parallel=1
        fi

        env_override="''${NIXFIED_WORKFLOW_PARALLEL:-}"
        if [ -n "$env_override" ]; then
          if [ "$env_override" = "1" ]; then
            run_parallel=1
          elif [ "$env_override" = "0" ]; then
            run_parallel=0
          else
            echo "WARN: ignoring invalid NIXFIED_WORKFLOW_PARALLEL='$env_override' (expected 0 or 1)"
          fi
        fi

        printf '%s' "$run_parallel"
      }

      workflow_unit_records() {
        local workflow_id="$1"
        workflow_plan_records "$workflow_id"
      }

      run_workflow_serial_impl() {
        local run_id="$1"
        local workflow_id="$2"
        local fail_fast="$3"
        shift 3
        local -a passthrough_args
        passthrough_args=("$@")
        local -A blocked_tasks_by_dependency
        local -A blocked_tasks_reason_by_dependency

        local status=0
        local workflow_status=0
        local unit_json

        while IFS= read -r unit_json; do
          local unit_task
          local unit_skip_service=""
          local blocked_by_dependency
          local dependency
          local failed_dependency=""
          local blocked_reason=""
          local missing=""
          local unit_selected_services_csv=""

          unit_task="$(workflow_unit_task_id "$unit_json")"
          missing="$(workflow_unit_missing_env_csv "$unit_json")"

          if [ -n "''${blocked_tasks_by_dependency[$unit_task]:-}" ]; then
            local detail_json
            blocked_reason="''${blocked_tasks_reason_by_dependency[$unit_task]:-dependency-not-passed}"
            local cascade_reason="dependency-not-passed"
            case "$blocked_reason" in
              service-skipped|missing-env|when-false|dependency-skipped)
                cascade_reason="dependency-skipped"
                ;;
            esac
            detail_json="$(event_detail_reason_key_value_json "$cascade_reason" "dependency" "''${blocked_tasks_by_dependency[$unit_task]}")"
            append_event "$run_id" "$workflow_id" "$unit_task" "canceled" "$detail_json" "$cascade_reason"
            continue
          fi

          blocked_by_dependency=0
          failed_dependency=""
          while IFS= read -r dependency; do
            if [ -z "''${blocked_tasks_by_dependency[$dependency]:-}" ]; then
              continue
            fi
            local detail_json
            failed_dependency="$dependency"
            blocked_reason="''${blocked_tasks_reason_by_dependency[$dependency]:-dependency-not-passed}"
            local cascade_reason="dependency-not-passed"
            case "$blocked_reason" in
              service-skipped|missing-env|when-false|dependency-skipped)
                cascade_reason="dependency-skipped"
                ;;
            esac
            detail_json="$(event_detail_reason_key_value_json "$cascade_reason" "dependency" "$failed_dependency")"
            append_event "$run_id" "$workflow_id" "$unit_task" "canceled" "$detail_json" "$cascade_reason"
            blocked_by_dependency=1
            break
          done < <(workflow_unit_dependencies "$unit_json")
          if [ "$blocked_by_dependency" -eq 1 ]; then
            blocked_tasks_by_dependency[$unit_task]="$failed_dependency"
            blocked_tasks_reason_by_dependency[$unit_task]="$blocked_reason"
            continue
          fi

          if [ -n "$missing" ]; then
            local detail_json
            detail_json="$(event_detail_reason_key_value_json "missing-env" "missing" "$missing")"
            append_event "$run_id" "$workflow_id" "$unit_task" "canceled" "$detail_json" "missing-env"
            blocked_tasks_by_dependency["$unit_task"]="$unit_task"
            blocked_tasks_reason_by_dependency["$unit_task"]="missing-env"
            continue
          fi

          unit_skip_service="$(workflow_unit_first_skipped_required_service "$unit_json" || true)"
          if [ -n "$unit_skip_service" ]; then
            local detail_json
            detail_json="$(event_detail_reason_key_value_json "service-skipped" "serviceName" "$unit_skip_service")"
            append_event "$run_id" "$workflow_id" "$unit_task" "canceled" "$detail_json" "service-skipped"
            echo "SKIP: task '$unit_task' (service '$unit_skip_service') is skipped because service '$unit_skip_service' has a skip flag enabled"
            blocked_tasks_by_dependency["$unit_task"]="$unit_task"
            blocked_tasks_reason_by_dependency["$unit_task"]="service-skipped"
            continue
          fi

          if ! workflow_unit_when_matches "$unit_json"; then
            local detail_json
            detail_json="$(event_detail_reason_json "when-false")"
            append_event "$run_id" "$workflow_id" "$unit_task" "canceled" "$detail_json" "when-false"
            blocked_tasks_by_dependency["$unit_task"]="$unit_task"
            blocked_tasks_reason_by_dependency["$unit_task"]="when-false"
            continue
          fi

          unit_selected_services_csv="$(workflow_unit_selected_services_csv "$unit_json")"
          if [ "''${#passthrough_args[@]}" -gt 0 ]; then
            if NIXFIED_SELECTED_SERVICES_CSV_OVERRIDE="$unit_selected_services_csv" execute_task "$run_id" "$workflow_id" "$unit_task" "''${passthrough_args[@]}"; then
              status=0
            else
              status="$?"
            fi
          elif NIXFIED_SELECTED_SERVICES_CSV_OVERRIDE="$unit_selected_services_csv" execute_task "$run_id" "$workflow_id" "$unit_task"; then
            status=0
          else
            status="$?"
          fi

          if [ "$status" -ne 0 ] && [ "$workflow_status" -eq 0 ]; then
            workflow_status="$status"
          fi

          if [ "$status" -ne 0 ] && [ "$fail_fast" = "true" ]; then
            break
          fi
        done < <(workflow_unit_records "$workflow_id")

        if [ "$workflow_status" -ne 0 ]; then
          return "$workflow_status"
        fi
        return 0
      }

      run_workflow_parallel_impl() {
        local run_id="$1"
        local workflow_id="$2"
        local fail_fast="$3"
        shift 3
        local -a passthrough_args
        passthrough_args=("$@")

        local max_workers
        local lock_policy
        local workflow_status=0
        local stop_scheduling=0
        local completed_count=0
        local running_count=0

        local -a unit_names
        unit_names=()

        local -A UNIT_JSON
        local -A UNIT_TASK
        local -A UNIT_NEEDS_LEFT
        local -A UNIT_STATE
        local -A UNIT_DEPENDENTS
        local -A UNIT_LOCKS
        local -A UNIT_PID
        local -A PID_UNIT
        local -A LOCK_OWNER
        local -A CANCEL_REQUESTED

        mark_unit_canceled() {
          local unit_name="$1"
          local reason="$2"
          local extra_key="''${3:-}"
          local extra_value="''${4:-}"
          local current_state
          local detail_json

          current_state="''${UNIT_STATE[$unit_name]:-pending}"
          if [ "$current_state" != "pending" ] && [ "$current_state" != "ready" ]; then
            return 0
          fi

          if [ -n "$extra_key" ]; then
            detail_json="$(event_detail_reason_key_value_json "$reason" "$extra_key" "$extra_value")"
          else
            detail_json="$(event_detail_reason_json "$reason")"
          fi

          append_event "$run_id" "$workflow_id" "''${UNIT_TASK[$unit_name]}" "canceled" "$detail_json" "$reason"
          UNIT_STATE[$unit_name]="canceled"
          completed_count=$((completed_count + 1))
        }

        cancel_pending_dependents() {
          local source_unit="$1"
          local reason="$2"
          local -a queue
          local current
          local dependent
          queue=("$source_unit")

          while [ "''${#queue[@]}" -gt 0 ]; do
            current="''${queue[0]}"
            queue=("''${queue[@]:1}")
            for dependent in ''${UNIT_DEPENDENTS[$current]:-}; do
              local before_state
              before_state="''${UNIT_STATE[$dependent]:-pending}"
              mark_unit_canceled "$dependent" "$reason" "dependency" "$current"
              if [ "$before_state" = "pending" ] || [ "$before_state" = "ready" ]; then
                queue+=("$dependent")
              fi
            done
          done
        }

        cancel_pending_units() {
          local reason="$1"
          local unit_name
          for unit_name in "''${unit_names[@]}"; do
            mark_unit_canceled "$unit_name" "$reason"
          done
        }

        unit_has_lock_conflict() {
          local unit_name="$1"
          local lock
          local owner
          for lock in ''${UNIT_LOCKS[$unit_name]:-}; do
            owner="''${LOCK_OWNER[$lock]:-}"
            if [ -n "$owner" ] && [ "$owner" != "$unit_name" ]; then
              return 0
            fi
          done
          return 1
        }

        assign_unit_locks() {
          local unit_name="$1"
          local lock
          for lock in ''${UNIT_LOCKS[$unit_name]:-}; do
            LOCK_OWNER[$lock]="$unit_name"
          done
        }

        release_unit_locks() {
          local unit_name="$1"
          local lock
          for lock in ''${UNIT_LOCKS[$unit_name]:-}; do
            if [ "''${LOCK_OWNER[$lock]:-}" = "$unit_name" ]; then
              unset "LOCK_OWNER[$lock]"
            fi
          done
        }

        next_ready_unit() {
          local unit_name
          for unit_name in "''${unit_names[@]}"; do
            if [ "''${UNIT_STATE[$unit_name]:-pending}" != "ready" ]; then
              continue
            fi
            if unit_has_lock_conflict "$unit_name"; then
              continue
            fi
            printf '%s' "$unit_name"
            return 0
          done
          return 1
        }

        start_unit() {
          local unit_name="$1"
          local unit_task
          local detail_json
          local pid
          local unit_selected_services_csv

          unit_task="''${UNIT_TASK[$unit_name]}"
          unit_selected_services_csv="$(workflow_unit_selected_services_csv "''${UNIT_JSON[$unit_name]}")"
          detail_json="$(event_detail_mode_json "task")"
          append_event "$run_id" "$workflow_id" "$unit_task" "queued" "$detail_json"
          append_event "$run_id" "$workflow_id" "$unit_task" "running" "$detail_json"

          if [ "''${#passthrough_args[@]}" -gt 0 ]; then
            (
              NIXFIED_TASK_ID="$unit_task" NIXFIED_PARENT_WORKFLOW_ID="$workflow_id" NIXFIED_SELECTED_SERVICES_CSV="$unit_selected_services_csv" execute_task_body "$unit_task" "''${passthrough_args[@]}"
            ) &
          else
            (
              NIXFIED_TASK_ID="$unit_task" NIXFIED_PARENT_WORKFLOW_ID="$workflow_id" NIXFIED_SELECTED_SERVICES_CSV="$unit_selected_services_csv" execute_task_body "$unit_task"
            ) &
          fi
          pid="$!"

          UNIT_STATE[$unit_name]="running"
          UNIT_PID[$unit_name]="$pid"
          PID_UNIT[$pid]="$unit_name"
          CANCEL_REQUESTED[$unit_name]=0
          assign_unit_locks "$unit_name"
          running_count=$((running_count + 1))
        }

        cancel_running_units() {
          local pid
          local unit_name

          for pid in "''${!PID_UNIT[@]}"; do
            unit_name="''${PID_UNIT[$pid]:-}"
            if [ -z "$unit_name" ]; then
              continue
            fi
            CANCEL_REQUESTED[$unit_name]=1
            kill -TERM "$pid" 2>/dev/null || true
          done

          sleep "$NIXFIED_RETRY_INTERVAL_DEFAULT"
          for pid in "''${!PID_UNIT[@]}"; do
            if kill -0 "$pid" 2>/dev/null; then
              kill -KILL "$pid" 2>/dev/null || true
            fi
          done
        }

        if max_workers="$(resolve_effective_max_workers "$workflow_id")"; then
          :
        else
          return "$?"
        fi
        lock_policy="$(workflow_lock_policy "$workflow_id")"
        if [ "$lock_policy" = "shared-aware" ]; then
          echo "WARN: lockPolicy=shared-aware uses exclusive semantics in workflow parallel runner"
        fi

        while IFS= read -r unit_json; do
          local unit_name
          local unit_task
          local needs_count
          local lock_list

          unit_name="$(workflow_unit_name "$unit_json")"
          unit_task="$(workflow_unit_task_id "$unit_json")"
          needs_count="$(workflow_unit_needs_count "$unit_json")"
          lock_list="$(workflow_unit_lock_list "$unit_json")"

          unit_names+=("$unit_name")
          UNIT_JSON[$unit_name]="$unit_json"
          UNIT_TASK[$unit_name]="$unit_task"
          UNIT_NEEDS_LEFT[$unit_name]="$needs_count"
          UNIT_STATE[$unit_name]="pending"
          UNIT_DEPENDENTS[$unit_name]=""
          UNIT_LOCKS[$unit_name]="$lock_list"
        done < <(workflow_unit_records "$workflow_id")

        local total_units="''${#unit_names[@]}"
        if [ "$total_units" -eq 0 ]; then
          return 0
        fi

        local unit_name
        for unit_name in "''${unit_names[@]}"; do
          while IFS= read -r dependency; do
            if [ -n "$dependency" ]; then
              UNIT_DEPENDENTS[$dependency]="''${UNIT_DEPENDENTS[$dependency]:-} $unit_name"
            fi
          done < <(workflow_unit_dependencies "''${UNIT_JSON[$unit_name]}")
        done

        for unit_name in "''${unit_names[@]}"; do
          local unit_json
          local unit_task
          local missing=""
          local unit_skip_service=""

          unit_json="''${UNIT_JSON[$unit_name]}"
          unit_task="$(workflow_unit_task_id "$unit_json")"
          missing="$(workflow_unit_missing_env_csv "$unit_json")"

          if [ -n "$missing" ]; then
            mark_unit_canceled "$unit_name" "missing-env" "missing" "$missing"
            cancel_pending_dependents "$unit_name" "dependency-skipped"
            continue
          fi

          unit_skip_service="$(workflow_unit_first_skipped_required_service "$unit_json" || true)"
          if [ -n "$unit_skip_service" ]; then
            mark_unit_canceled "$unit_name" "service-skipped" "serviceName" "$unit_skip_service"
            cancel_pending_dependents "$unit_name" "dependency-skipped"
            continue
          fi

          if ! workflow_unit_when_matches "$unit_json"; then
            mark_unit_canceled "$unit_name" "when-false"
            cancel_pending_dependents "$unit_name" "dependency-skipped"
            continue
          fi

          if [ "''${UNIT_NEEDS_LEFT[$unit_name]}" -eq 0 ]; then
            UNIT_STATE[$unit_name]="ready"
          fi
        done

        while [ "$completed_count" -lt "$total_units" ]; do
          if [ "$stop_scheduling" -eq 0 ]; then
            while [ "$running_count" -lt "$max_workers" ]; do
              local ready_unit
              ready_unit="$(next_ready_unit || true)"
              if [ -z "$ready_unit" ]; then
                break
              fi
              start_unit "$ready_unit"
            done
          fi

          if [ "$running_count" -eq 0 ]; then
            if [ "$completed_count" -lt "$total_units" ] && [ "$stop_scheduling" -eq 0 ]; then
              cancel_pending_units "blocked"
              if [ "$workflow_status" -eq 0 ]; then
                workflow_status=1
              fi
            fi
            break
          fi

          local done_pid=""
          local wait_rc
          local done_unit
          local detail_json

          if wait -n -p done_pid; then
            wait_rc=0
          else
            wait_rc="$?"
          fi

          done_unit="''${PID_UNIT[$done_pid]:-}"
          if [ -z "$done_unit" ]; then
            continue
          fi

          unset "PID_UNIT[$done_pid]"
          unset "UNIT_PID[$done_unit]"
          running_count=$((running_count - 1))
          release_unit_locks "$done_unit"

          if [ "''${CANCEL_REQUESTED[$done_unit]:-0}" = "1" ]; then
            detail_json="$(event_detail_reason_json "fail-fast-running")"
            append_event "$run_id" "$workflow_id" "''${UNIT_TASK[$done_unit]}" "canceled" "$detail_json" "fail-fast-running"
            UNIT_STATE[$done_unit]="canceled"
            completed_count=$((completed_count + 1))
            continue
          fi

          if [ "$wait_rc" -eq 0 ]; then
            detail_json="$(event_detail_produces_json "$(workflow_unit_produces_json "''${UNIT_JSON[$done_unit]}")")"
            append_event "$run_id" "$workflow_id" "''${UNIT_TASK[$done_unit]}" "passed" "$detail_json"
            UNIT_STATE[$done_unit]="passed"
            completed_count=$((completed_count + 1))

            local dependent
            for dependent in ''${UNIT_DEPENDENTS[$done_unit]:-}; do
              if [ "''${UNIT_STATE[$dependent]:-pending}" = "pending" ]; then
                UNIT_NEEDS_LEFT[$dependent]="$(( ''${UNIT_NEEDS_LEFT[$dependent]} - 1 ))"
                if [ "''${UNIT_NEEDS_LEFT[$dependent]}" -eq 0 ]; then
                  UNIT_STATE[$dependent]="ready"
                fi
              fi
            done
          else
            detail_json="$(event_detail_exit_code_json "$wait_rc")"
            append_event "$run_id" "$workflow_id" "''${UNIT_TASK[$done_unit]}" "failed" "$detail_json" "" "$wait_rc"
            UNIT_STATE[$done_unit]="failed"
            completed_count=$((completed_count + 1))

            if [ "$workflow_status" -eq 0 ]; then
              workflow_status="$wait_rc"
            fi

            if [ "$fail_fast" = "true" ] && [ "$stop_scheduling" -eq 0 ]; then
              stop_scheduling=1
              cancel_running_units
              cancel_pending_units "fail-fast"
            else
              cancel_pending_dependents "$done_unit" "dependency-not-passed"
            fi
          fi
        done

        return "$workflow_status"
      }

      run_workflow_phase_tasks() {
        local run_id="$1"
        local workflow_id="$2"
        local phase_key="$3"
        shift 3
        local -a passthrough_args
        passthrough_args=("$@")

        local phase_task
        local phase_status=0

        while IFS= read -r phase_task; do
          local phase_task_skip_service
          local phase_skip_detail
          local phase_task_selected_services_csv=""

          if [ -z "$phase_task" ]; then
            continue
          fi

          case "$phase_task" in
            task.ops.ready|task.ops.health)
              phase_task_selected_services_csv="$(
                workflow_unit_closure_selected_services "$workflow_id" | selected_services_csv_from_lines
              )"
              ;;
          esac

          phase_task_skip_service="$(task_first_skipped_required_service "$phase_task" || true)"
          if [ -n "$phase_task_skip_service" ]; then
            phase_skip_detail="$(event_detail_reason_key_value_json "service-skipped" "serviceName" "$phase_task_skip_service")"
            append_event "$run_id" "$workflow_id" "$phase_task" "canceled" "$phase_skip_detail" "service-skipped"
            echo "SKIP: task '$phase_task' (service '$phase_task_skip_service') is skipped because service '$phase_task_skip_service' has a skip flag enabled"
            continue
          fi

          if [ "$phase_task" = "task.ops.ready" ] || [ "$phase_task" = "task.ops.health" ]; then
            if [ "''${#passthrough_args[@]}" -gt 0 ]; then
              if NIXFIED_SELECTED_SERVICES_CSV_OVERRIDE="$phase_task_selected_services_csv" execute_task "$run_id" "$workflow_id" "$phase_task" "''${passthrough_args[@]}"; then
                phase_status=0
              else
                phase_status="$?"
                break
              fi
            elif NIXFIED_SELECTED_SERVICES_CSV_OVERRIDE="$phase_task_selected_services_csv" execute_task "$run_id" "$workflow_id" "$phase_task"; then
              phase_status=0
            else
              phase_status="$?"
              break
            fi
          elif [ "''${#passthrough_args[@]}" -gt 0 ] && execute_task "$run_id" "$workflow_id" "$phase_task" "''${passthrough_args[@]}"; then
            phase_status=0
          elif execute_task "$run_id" "$workflow_id" "$phase_task"; then
            phase_status=0
          else
            phase_status="$?"
            break
          fi
        done < <(workflow_phase_tasks "$workflow_id" "$phase_key")

        return "$phase_status"
      }

      run_workflow_phase_service_sets() {
        local run_id="$1"
        local workflow_id="$2"
        local phase_key="$3"
        local phase_status=0
        local phase_entry_tsv=""
        local service_set_id=""
        local service_set_name=""
        local operation=""
        local phase_entry_id=""
        local selected_services_csv=""
        local phase_detail=""
        local failure_detail=""
        local program_path=""

        while IFS=$'\t' read -r service_set_id service_set_name operation selected_services_csv; do
          if [ -z "$service_set_id" ]; then
            continue
          fi

          phase_entry_id="''${service_set_id}:''${operation}"
          phase_detail="$(workflow_phase_service_set_detail_json "$phase_key" "$service_set_id" "$service_set_name" "$operation")"

          program_path="$(workflow_phase_service_set_program "$service_set_id" "$operation" || true)"
          if [ -z "$program_path" ]; then
            echo "ERROR: missing service-set program serviceSetId=$service_set_id operation=$operation" >&2
            append_event "$run_id" "$workflow_id" "$phase_entry_id" "failed" "$phase_detail"
            phase_status=1
            break
          fi

          append_event "$run_id" "$workflow_id" "$phase_entry_id" "queued" "$phase_detail"
          append_event "$run_id" "$workflow_id" "$phase_entry_id" "running" "$phase_detail"

          if NIXFIED_SELECTED_SERVICES_CSV="$selected_services_csv" "$program_path"; then
            append_event "$run_id" "$workflow_id" "$phase_entry_id" "passed" "$phase_detail"
            phase_status=0
          else
            phase_status="$?"
            failure_detail="$(workflow_phase_service_set_failure_json "$phase_key" "$service_set_id" "$service_set_name" "$operation" "$phase_status")"
            append_event "$run_id" "$workflow_id" "$phase_entry_id" "failed" "$failure_detail" "" "$phase_status"
            break
          fi
        done < <(workflow_phase_service_sets "$workflow_id" "$phase_key")

        return "$phase_status"
      }

      run_workflow_phase() {
        local run_id="$1"
        local workflow_id="$2"
        local phase_key="$3"
        shift 3
        local -a passthrough_args
        passthrough_args=("$@")

        if [ "$phase_key" = "preRun" ]; then
          if run_workflow_phase_service_sets "$run_id" "$workflow_id" "$phase_key"; then
            run_workflow_phase_tasks "$run_id" "$workflow_id" "$phase_key" "''${passthrough_args[@]}"
          else
            return "$?"
          fi
        else
          if run_workflow_phase_tasks "$run_id" "$workflow_id" "$phase_key" "''${passthrough_args[@]}"; then
            run_workflow_phase_service_sets "$run_id" "$workflow_id" "$phase_key"
          else
            return "$?"
          fi
        fi
      }

      is_nonneg_int() {
        case "''${1:-}" in
          ""|*[!0-9]*)
            return 1
            ;;
          *)
            return 0
            ;;
        esac
      }

      workflow_setup_timing_fields() {
        local started_epoch="$1"
        local started_at="$2"
        local summary_started_epoch="$started_epoch"
        local summary_started_at="$started_at"
        local setup_duration=0
        local setup_started_epoch="''${NIXFIED_WORKFLOW_SETUP_STARTED_EPOCH:-}"
        local setup_started_at="''${NIXFIED_WORKFLOW_SETUP_STARTED_AT:-}"

        if is_nonneg_int "$setup_started_epoch" && [ "$setup_started_epoch" -le "$started_epoch" ]; then
          summary_started_epoch="$setup_started_epoch"
          setup_duration="$(( started_epoch - setup_started_epoch ))"
          if [ -n "$setup_started_at" ]; then
            summary_started_at="$setup_started_at"
          fi
        fi

        printf '%s\t%s\t%s' "$summary_started_epoch" "$summary_started_at" "$setup_duration"
      }

      format_duration_seconds() {
        local seconds="$1"
        if ! is_nonneg_int "$seconds"; then
          printf '%s' "?"
          return 0
        fi

        if [ "$seconds" -lt 60 ]; then
          printf '%s' "''${seconds}s"
          return 0
        fi

        local mins
        local secs
        mins="$(( seconds / 60 ))"
        secs="$(( seconds % 60 ))"
        printf '%s' "''${mins}m ''${secs}s"
      }

      workflow_collect_steps() {
        local run_id="$1"
        local events_index_file="$2"
        local steps_target="$3"
        local actual_steps_target="$steps_target"
        local cleanup_steps_target=0
        local export_file=""
        local attempt_id="''${NIXFIED_ATTEMPT_ID:-}"

        WORKFLOW_PASSED_COUNT=0
        WORKFLOW_FAILED_COUNT=0
        WORKFLOW_SKIPPED_COUNT=0
        WORKFLOW_CANCELED_COUNT=0
        WORKFLOW_STEPS_DURATION=0
        WORKFLOW_PEAK_WORKERS=0
        WORKFLOW_LEAF_TASK_IDS_LINES=""

        if [ ! -f "$events_index_file" ]; then
          if [ -n "$steps_target" ]; then
            : > "$steps_target" || return 1
          fi
          return 0
        fi

        if [ -z "$actual_steps_target" ]; then
          actual_steps_target="$(mktemp "''${TMPDIR:-/tmp}/nixfied-summary-steps-kernel.XXXXXX")" || {
            echo "ERROR: failed to create workflow summary steps temp file"
            return 1
          }
          cleanup_steps_target=1
        else
          : > "$actual_steps_target" || return 1
        fi

        export_file="$(mktemp "''${TMPDIR:-/tmp}/nixfied-summary-exports.XXXXXX")" || {
          if [ "$cleanup_steps_target" -eq 1 ]; then
            rm -f "$actual_steps_target"
          fi
          echo "ERROR: failed to create workflow summary export temp file"
          return 1
        }

        if ! ${kernelPackage}/bin/nixfied-kernel summary collect-steps \
          ${pkgs.lib.escapeShellArg (builtins.toString workflowSummaryPlanFile)} \
          "$events_index_file" \
          "$run_id" \
          "$attempt_id" \
          "$actual_steps_target" \
          "$export_file" \
          >/dev/null; then
          rm -f "$export_file"
          if [ "$cleanup_steps_target" -eq 1 ]; then
            rm -f "$actual_steps_target"
          fi
          return 1
        fi

        if ! . "$export_file"; then
          rm -f "$export_file"
          if [ "$cleanup_steps_target" -eq 1 ]; then
            rm -f "$actual_steps_target"
          fi
          echo "ERROR: failed to load workflow summary exports"
          return 1
        fi

        rm -f "$export_file"
        if [ "$cleanup_steps_target" -eq 1 ]; then
          rm -f "$actual_steps_target"
        fi
      }

      workflow_steps_json() {
        printf '%s' "[]"
      }

      print_workflow_summary_report() {
        local run_id="$1"
        local workflow_id="$2"
        local exit_code="$3"
        local duration_seconds="$4"
        local summary_file="$5"
        local events_index_file=""
        local steps_display_file=""
        local steps_tmp=""
        local step_name=""
        local step_status=""
        local step_duration=""
        local step_marker=""

        if [ -n "$summary_file" ] && [ -f "$summary_file" ]; then
          if ${kernelPackage}/bin/nixfied-kernel summary render-human "$summary_file"; then
            return 0
          fi
          echo "WARN: failed to render summary envelope '$summary_file'; falling back to registry-derived report"
        fi

        echo ""
        echo "------------------------------------------------------------"
        echo "Summary"
        echo "------------------------------------------------------------"

        events_index_file="$(registry_events_index_snapshot "$REGISTRY_ROOT" 2>/dev/null || true)"
        if [ -n "$events_index_file" ] && [ -f "$events_index_file" ]; then
          steps_tmp="$(mktemp "''${TMPDIR:-/tmp}/nixfied-summary-steps.XXXXXX")" || true
          if [ -n "$steps_tmp" ] && workflow_collect_steps "$run_id" "$events_index_file" "$steps_tmp" 2>/dev/null; then
            steps_display_file="$steps_tmp"
          fi
        fi

        if [ -n "$steps_display_file" ] && [ -f "$steps_display_file" ]; then
          while IFS=$'\t' read -r step_name step_status step_duration; do
            [ -n "$step_name" ] || continue
            case "$step_status" in
              passed)
                step_marker="PASS"
                ;;
              skipped)
                step_marker="SKIP"
                ;;
              *)
                step_marker="FAIL"
                ;;
            esac
            echo "  [$step_marker] $step_name (''${step_duration:-?}s)"
          done < "$steps_display_file"
        fi

        if is_nonneg_int "$duration_seconds"; then
          echo "Total time: $(format_duration_seconds "$duration_seconds")"
        fi

        if [ "$exit_code" -eq 0 ]; then
          echo "OK: Exit code: 0"
        else
          echo "ERROR: Exit code: $exit_code"
        fi

        if [ -n "$summary_file" ] && [ -f "$summary_file" ]; then
          echo ""
          echo "Artifacts: $(dirname "$summary_file")"
        fi

        echo "------------------------------------------------------------"
        rm -f "$steps_tmp"
        registry_snapshot_cleanup "$events_index_file"
      }

      write_workflow_summary_json() {
        local run_id="$1"
        local workflow_id="$2"
        local exit_code="$3"
        local started_at="$4"
        local started_epoch="$5"
        local summary_file_override="$6"

        local should_write
        local mode
        local artifacts_dir
        local summary_file
        local summary_tmp
        local summary_steps_tmp
        local summary_started_at="$started_at"
        local summary_started_epoch="$started_epoch"
        local finished_at
        local duration_seconds
        local passed
        local failed
        local skipped
        local canceled
        local steps_duration
        local setup_duration=0
        local teardown_duration=0
        local accounted_duration
        local untracked_duration
        local parallel_max_workers
        local parallel_peak_workers
        local parallel_canceled_count
        local leaf_task_ids_lines=""
        local events_index_file=""
        local setup_timing_fields
        local attempt_id="''${NIXFIED_ATTEMPT_ID:-}"

        LAST_WORKFLOW_SUMMARY_FILE=""
        LAST_WORKFLOW_ATTEMPT_ID="$attempt_id"
        LAST_WORKFLOW_SUMMARY_PASSED_COUNT="0"
        LAST_WORKFLOW_SUMMARY_FAILED_COUNT="0"
        LAST_WORKFLOW_SUMMARY_SKIPPED_COUNT="0"
        LAST_WORKFLOW_SUMMARY_CANCELED_COUNT="0"

        should_write="$(workflow_write_summary "$workflow_id")"
        if [ "$should_write" != "true" ]; then
          return 0
        fi

        mode="$(workflow_mode_name "$workflow_id")"
        artifacts_dir="''${CI_ARTIFACTS_DIR:-}"
        if [ -z "$artifacts_dir" ]; then
          echo "ERROR: CI_ARTIFACTS_DIR is not set for run '$run_id'"
          return 1
        fi
        summary_file="$artifacts_dir/summary.json"

        if ! mkdir -p "$artifacts_dir"; then
          echo "ERROR: failed to create artifacts directory '$artifacts_dir'"
          return 1
        fi

        setup_timing_fields="$(workflow_setup_timing_fields "$started_epoch" "$started_at")"
        IFS=$'\t' read -r summary_started_epoch summary_started_at setup_duration <<< "$setup_timing_fields"

        finished_at="$(date -u +"%Y-%m-%dT%H:%M:%SZ")"
        duration_seconds="$(( $(date +%s) - summary_started_epoch ))"
        if [ "$duration_seconds" -lt 0 ]; then
          duration_seconds=0
        fi

        events_index_file="$(registry_events_index_snapshot "$REGISTRY_ROOT" 2>/dev/null || true)"
        summary_steps_tmp="$(mktemp "''${TMPDIR:-/tmp}/nixfied-summary-steps-write.XXXXXX")" || {
          registry_snapshot_cleanup "$events_index_file"
          echo "ERROR: failed to create workflow summary steps temp file"
          return 1
        }
        if [ -n "$events_index_file" ] && [ -f "$events_index_file" ]; then
          if workflow_collect_steps "$run_id" "$events_index_file" "$summary_steps_tmp"; then
            passed="$WORKFLOW_PASSED_COUNT"
            failed="$WORKFLOW_FAILED_COUNT"
            skipped="$WORKFLOW_SKIPPED_COUNT"
            canceled="$WORKFLOW_CANCELED_COUNT"
            steps_duration="$WORKFLOW_STEPS_DURATION"
            parallel_peak_workers="$WORKFLOW_PEAK_WORKERS"
            leaf_task_ids_lines="$WORKFLOW_LEAF_TASK_IDS_LINES"
          else
            echo "WARN: failed to collect step summary from '$events_index_file'; using empty step list"
            passed=0
            failed=0
            skipped=0
            canceled=0
            steps_duration=0
            parallel_peak_workers=0
            leaf_task_ids_lines=""
            : > "$summary_steps_tmp"
          fi
        else
          passed=0
          failed=0
          skipped=0
          canceled=0
          steps_duration=0
          parallel_peak_workers=0
          leaf_task_ids_lines=""
          : > "$summary_steps_tmp"
        fi
        accounted_duration="$(( setup_duration + steps_duration + teardown_duration ))"
        untracked_duration="$(( duration_seconds - accounted_duration ))"
        if [ "$untracked_duration" -lt 0 ]; then
          untracked_duration=0
        fi

        if parallel_max_workers="$(resolve_effective_max_workers "$workflow_id" 2>/dev/null || true)"; then
          :
        fi
        if ! is_nonneg_int "$parallel_max_workers"; then
          parallel_max_workers=""
        fi

        if ! is_nonneg_int "$parallel_peak_workers"; then
          parallel_peak_workers=""
        fi

        parallel_canceled_count="$canceled"
        if ! is_nonneg_int "$parallel_canceled_count"; then
          parallel_canceled_count=""
        fi

        if ! ${kernelPackage}/bin/nixfied-kernel summary compose \
          ${lib.escapeShellArg validationBundleFile} \
          "$summary_file" \
          "$run_id" \
          "$attempt_id" \
          "$workflow_id" \
          "$mode" \
          "$exit_code" \
          "$summary_started_at" \
          "$finished_at" \
          "$duration_seconds" \
          "$passed" \
          "$failed" \
          "$skipped" \
          "$canceled" \
          "$summary_steps_tmp" \
          "$duration_seconds" \
          "$setup_duration" \
          "$steps_duration" \
          "$teardown_duration" \
          "$accounted_duration" \
          "$untracked_duration" \
          "$parallel_max_workers" \
          "$parallel_peak_workers" \
          "$parallel_canceled_count" >/dev/null; then
          rm -f "$summary_steps_tmp"
          registry_snapshot_cleanup "$events_index_file"
          echo "ERROR: failed to validate summary file '$summary_file'"
          return 1
        fi
        rm -f "$summary_steps_tmp"

        if [ -n "$summary_file_override" ] && [ "$summary_file_override" != "$summary_file" ]; then
          if ! copy_file_atomic "$summary_file" "$summary_file_override"; then
            registry_snapshot_cleanup "$events_index_file"
            echo "ERROR: failed to write summary file '$summary_file_override'"
            return 1
          fi
        fi

        LAST_WORKFLOW_SUMMARY_FILE="$summary_file"
        LAST_WORKFLOW_SUMMARY_PASSED_COUNT="$passed"
        LAST_WORKFLOW_SUMMARY_FAILED_COUNT="$failed"
        LAST_WORKFLOW_SUMMARY_SKIPPED_COUNT="$skipped"
        LAST_WORKFLOW_SUMMARY_CANCELED_COUNT="$canceled"
        echo "INFO: summary_json=$summary_file"
        registry_snapshot_cleanup "$events_index_file"
        return 0
      }

      run_workflow() {
        if [ "$#" -lt 1 ]; then
          echo "ERROR: usage: run-workflow <workflow-id> [-- ...]"
          return "$NIXFIED_EXIT_USAGE"
        fi

        local workflow_id="$1"
        shift

        local mode_override=""
        local print_summary=0
        local parse_options=1
        local -a input_args
        input_args=()
        local -a passthrough_args
        passthrough_args=()
        local arg
        local shorthand_mode

        extract_logging_override_args "$@" || return $?
        input_args=("''${LOGGING_FILTERED_ARGS[@]}")
        extract_machine_output_args "''${input_args[@]}" || return $?
        input_args=("''${MACHINE_FILTERED_ARGS[@]}")
        set -- "''${input_args[@]}"

        while [ "$#" -gt 0 ]; do
          arg="$1"
          shift

          if [ "$parse_options" -eq 0 ]; then
            passthrough_args+=("$arg")
            continue
          fi

          case "$arg" in
            --mode)
              if [ "$#" -lt 1 ]; then
                echo "ERROR: --mode requires a value"
                return "$NIXFIED_EXIT_USAGE"
              fi
              mode_override="$1"
              shift
              ;;
            --mode=*)
              mode_override="''${arg#--mode=}"
              ;;
            --summary)
              print_summary=1
              ;;
            --)
              parse_options=0
              ;;
            --*)
              shorthand_mode="''${arg#--}"
              if workflow_simple_shorthand_exists_for_family "$workflow_id" "$shorthand_mode"; then
                mode_override="$shorthand_mode"
              else
                echo "ERROR: unknown option '$arg'"
                return "$NIXFIED_EXIT_USAGE"
              fi
              ;;
            -*)
              echo "ERROR: unknown option '$arg'"
              return "$NIXFIED_EXIT_USAGE"
              ;;
            *)
              passthrough_args+=("$arg")
              ;;
          esac
        done

        workflow_id="$(resolve_workflow_mode "$workflow_id" "$mode_override")" || return $?

        local run_id
        local detail_json
        local fail_fast
        local run_parallel
        local status=0
        local post_status=0
        local post_always=true
        local started_at
        local started_epoch
        local duration_seconds
        local summary_file=""
        local nested_workflow_call=0
        local managed_by_orchestrator=0
        local NIXFIED_WORKFLOW_CONTEXT="1"
        local NIXFIED_WORKFLOW_LOG_LEVEL_DEFAULT=""
        local NIXFIED_WORKFLOW_OUTPUT_MODE_DEFAULT=""

        if [ "''${NIXFIED_WORKFLOW_NESTED:-0}" = "1" ]; then
          nested_workflow_call=1
        fi

        if ! workflow_id_exists "$workflow_id"; then
          echo "ERROR: unknown workflow '$workflow_id'"
          return "$NIXFIED_EXIT_USAGE"
        fi
        NIXFIED_WORKFLOW_LOG_LEVEL_DEFAULT="$(workflow_logging_level_default "$workflow_id")"
        NIXFIED_WORKFLOW_OUTPUT_MODE_DEFAULT="$(workflow_logging_output_default "$workflow_id")"

        started_at="$(date -u +"%Y-%m-%dT%H:%M:%SZ")"
        started_epoch="$(date +%s)"

        if [ "''${NIXFIED_ORCHESTRATOR_MANAGED:-0}" = "1" ] && [ -n "''${NIXFIED_ORCHESTRATOR_RUN_ID:-}" ]; then
          run_id="$NIXFIED_ORCHESTRATOR_RUN_ID"
          attempt_id="''${NIXFIED_ORCHESTRATOR_ATTEMPT_ID:-}"
          RUN_SUFFIX_REASON="''${NIXFIED_ORCHESTRATOR_RUN_SUFFIX_REASON:-orchestrator}"
          managed_by_orchestrator=1
        else
          run_id="$(compute_run_id "workflow" "$workflow_id" "" "''${passthrough_args[@]}")"
          attempt_id="$(compute_attempt_id)"
          activate_run "$run_id"
          trap "deactivate_run '$run_id'" EXIT
        fi
        if [ -z "$attempt_id" ]; then
          attempt_id="$(compute_attempt_id)"
        fi

        if [ -n "$MACHINE_RUN_ID_FILE" ]; then
          write_text_file_atomic "$MACHINE_RUN_ID_FILE" "$run_id" || return $?
        fi

        ensure_run_artifacts_dir "$run_id" "$workflow_id" "$managed_by_orchestrator" || return $?
        export NIXFIED_RUN_ID="$run_id"
        export NIXFIED_ATTEMPT_ID="$attempt_id"

        detail_json="$(event_detail_mode_suffix_json "workflow" "$RUN_SUFFIX_REASON")"
        append_event "$run_id" "$workflow_id" "" "queued" "$detail_json"

        fail_fast="$(workflow_fail_fast "$workflow_id")"
        run_parallel="$(resolve_parallel_mode "$workflow_id")"
        if [ "$run_parallel" = "1" ]; then
          local parallel_cap_error=""
          if parallel_cap_error="$(parallel_worker_cap_override_error)"; then
            printf '%s\n' "$parallel_cap_error"
            return "$NIXFIED_EXIT_USAGE"
          fi
        fi

        if run_workflow_phase "$run_id" "$workflow_id" "preRun" "''${passthrough_args[@]}"; then
          status=0
        else
          status="$?"
        fi

        if [ "$status" -eq 0 ]; then
          if [ "$run_parallel" = "1" ]; then
            if run_workflow_parallel_impl "$run_id" "$workflow_id" "$fail_fast" "''${passthrough_args[@]}"; then
              status=0
            else
              status="$?"
            fi
          else
            if run_workflow_serial_impl "$run_id" "$workflow_id" "$fail_fast" "''${passthrough_args[@]}"; then
              status=0
            else
              status="$?"
            fi
          fi
        fi

        post_always="$(workflow_post_run_always "$workflow_id")"
        if [ "$post_always" = "true" ] || [ "$status" -eq 0 ]; then
          if run_workflow_phase "$run_id" "$workflow_id" "postRun" "''${passthrough_args[@]}"; then
            post_status=0
          else
            post_status="$?"
          fi

          if [ "$post_status" -ne 0 ] && [ "$status" -eq 0 ]; then
            status="$post_status"
          fi
        fi

        if [ "$status" -eq 0 ]; then
          append_event "$run_id" "$workflow_id" "" "passed" "$(event_detail_mode_json "workflow")"
        else
          detail_json="$(event_detail_exit_code_json "$status")"
          append_event "$run_id" "$workflow_id" "" "failed" "$detail_json" "" "$status"
        fi

        duration_seconds="$(( $(date +%s) - started_epoch ))"
        if [ "$duration_seconds" -lt 0 ]; then
          duration_seconds=0
        fi

        if [ "$nested_workflow_call" -eq 0 ]; then
          if ! write_workflow_summary_json "$run_id" "$workflow_id" "$status" "$started_at" "$started_epoch" "$MACHINE_SUMMARY_FILE"; then
            if [ "$status" -eq 0 ]; then
              status=1
              detail_json="$(event_detail_reason_exit_code_json "summary-write-failed" "1")"
              append_event "$run_id" "$workflow_id" "" "failed" "$detail_json" "summary-write-failed" "1"
            fi
          fi
          summary_file="$LAST_WORKFLOW_SUMMARY_FILE"
        fi

        if [ "$print_summary" -eq 1 ] && [ "$nested_workflow_call" -eq 0 ]; then
          local events_file=""
          local passed failed skipped canceled
          print_workflow_summary_report "$run_id" "$workflow_id" "$status" "$duration_seconds" "$summary_file"

          if [ -n "$summary_file" ] && [ -f "$summary_file" ]; then
            passed="''${LAST_WORKFLOW_SUMMARY_PASSED_COUNT:-0}"
            failed="''${LAST_WORKFLOW_SUMMARY_FAILED_COUNT:-0}"
            skipped="''${LAST_WORKFLOW_SUMMARY_SKIPPED_COUNT:-0}"
            canceled="''${LAST_WORKFLOW_SUMMARY_CANCELED_COUNT:-0}"
          else
            events_file="$(registry_events_index_snapshot "$REGISTRY_ROOT" 2>/dev/null || true)"
            if [ -n "$events_file" ] && [ -f "$events_file" ]; then
              if workflow_collect_steps "$run_id" "$events_file" "" 2>/dev/null; then
                passed="$WORKFLOW_PASSED_COUNT"
                failed="$WORKFLOW_FAILED_COUNT"
                skipped="$WORKFLOW_SKIPPED_COUNT"
                canceled="$WORKFLOW_CANCELED_COUNT"
              else
                passed=0
                failed=0
                skipped=0
                canceled=0
              fi
              registry_snapshot_cleanup "$events_file"
            else
              passed=0
              failed=0
              skipped=0
              canceled=0
            fi
          fi
          echo "INFO: runId=$run_id passed=$passed failed=$failed canceled=$canceled skipped=$skipped"
        fi

        if [ "$managed_by_orchestrator" -eq 0 ]; then
          trap - EXIT
          deactivate_run "$run_id"
        fi

        return "$status"
      }

      main() {
        if [ "$#" -lt 1 ]; then
          echo "ERROR: usage: nixfied-executor <run-task|run-workflow> ..."
          exit "$NIXFIED_EXIT_USAGE"
        fi

        local subcommand="$1"
        shift

        case "$subcommand" in
          run-task)
            run_task "$@"
            ;;
          run-workflow)
            run_workflow "$@"
            ;;
          *)
            echo "ERROR: unknown subcommand '$subcommand'"
            exit "$NIXFIED_EXIT_USAGE"
            ;;
        esac
      }

      main "$@"
''
