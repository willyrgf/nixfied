{
  lib,
  pkgs,
  conf,
  mkCommandTask,
  mkTaskLauncher,
  ownerFile ? "nixfied/framework/presets/framework-test.nix",
}:
let
  plainShellLogging = import ../core/plain-shell-logging.nix;
  shellCommon = import ../core/shell-common.nix { inherit pkgs; };
  frameworkTestShardCatalog = import ../../../tests/framework/framework-test-catalog.nix;
  frameworkTestShardNames = frameworkTestShardCatalog.order;
  frameworkTestProfileNames = frameworkTestShardCatalog.profileNames;
  renderShellArray =
    values: lib.concatMapStrings (value: "          ${lib.escapeShellArg value}\n") values;
  renderCheckArgsList =
    checkNames:
    lib.concatMapStringsSep " \\\n" (
      checkName: "            ${lib.escapeShellArg ".#checks.${pkgs.system}.${checkName}"}"
    ) checkNames;
  renderCheckArgs = shardName: renderCheckArgsList frameworkTestShardCatalog.checks.${shardName};
  renderProfileShardNames =
    profileName:
    builtins.filter (
      shardName: (frameworkTestShardCatalog.profileShardChecks.${profileName}.${shardName} or [ ]) != [ ]
    ) frameworkTestShardNames;
  renderProfileShardBody =
    profileName: shardName:
    let
      checkNames = frameworkTestShardCatalog.profileShardChecks.${profileName}.${shardName} or [ ];
      fullE2eTail =
        if profileName == "full" && shardName == "e2e" then
          ''
            nix run .#run-workflow -- workflow.test.framework.selfhost --summary
          ''
        else
          "";
    in
    if checkNames == [ ] then
      ''log_skip "shard has no selected checks profile=${profileName} name=${shardName}"''
    else
      ''
                    run_nix_build_shard \
        ${renderCheckArgsList checkNames}
        ${fullE2eTail}'';
  frameworkTestMaxParallelShardsRaw = conf.frameworkTest.maxParallelShards or "auto";
  frameworkTestMaxParallelShards =
    if builtins.isInt frameworkTestMaxParallelShardsRaw then
      toString frameworkTestMaxParallelShardsRaw
    else if builtins.isString frameworkTestMaxParallelShardsRaw then
      frameworkTestMaxParallelShardsRaw
    else
      throw "ERROR: frameworkTest.maxParallelShards must be \"auto\" or a positive integer";
in
{
  tasks = {
    framework-test = mkCommandTask {
      id = "task.framework.test";
      kind = "utility";
      summary = "Run framework validation in the model";
      description = ''
        Runs framework validation shards with configurable shard parallelism.
      '';
      runtimeInputs = [
        pkgs.bash
        pkgs.coreutils
        pkgs.findutils
        pkgs.gnugrep
        pkgs.gnused
        pkgs.nix
      ];
      passThroughEnv = [ "NIXFIED_FRAMEWORK_TEST_FORCE_FAIL_SHARD" ];
      contractArgs = [
        {
          name = "summary";
          kind = "flag";
          long = "--summary";
          description = "Print compact summary output.";
        }
        {
          name = "summary-json";
          kind = "option";
          long = "--summary-json";
          type = "string";
          description = "Write summary JSON to a file.";
        }
        {
          name = "profile";
          kind = "option";
          long = "--profile";
          type = "enum";
          values = frameworkTestProfileNames;
          description = "Check profile to run (feature-proof, ci, or full).";
        }
        {
          name = "shard";
          kind = "option";
          long = "--shard";
          type = "string";
          values = frameworkTestShardNames;
          description = "Run one shard only.";
        }
        {
          name = "max-parallel-shards";
          kind = "option";
          long = "--max-parallel-shards";
          type = "string";
          description = "Shard worker cap (positive integer) or 'auto' for all selected shards.";
        }
        {
          name = "serial";
          kind = "flag";
          long = "--serial";
          description = "Force serial shard execution.";
        }
        {
          name = "list-shards";
          kind = "flag";
          long = "--list-shards";
          description = "List available shards and exit.";
        }
      ];
      command = ''
                        set -euo pipefail

                        ROOT="$(pwd -P)"
                        PROFILE="ci"
                        SHARD=""
                        LIST_SHARDS=0
                        SUMMARY=0
                        SUMMARY_JSON=""
                        MAX_PARALLEL_SHARDS_DEFAULT=${lib.escapeShellArg frameworkTestMaxParallelShards}
                        MAX_PARALLEL_SHARDS="$MAX_PARALLEL_SHARDS_DEFAULT"
                        SERIAL=0
                        SHARDS=(
                ${renderShellArray frameworkTestShardNames}        )
                        PROFILE_FEATURE_PROOF_SHARDS=(
                ${renderShellArray (renderProfileShardNames "feature-proof")}        )
                        PROFILE_CI_SHARDS=(
                ${renderShellArray (renderProfileShardNames "ci")}        )
                        PROFILE_FULL_SHARDS=(
                ${renderShellArray (renderProfileShardNames "full")}        )
                        EXECUTED=0
                        FAILED_SHARDS=0
                        EXIT_1_SHARDS=0
                        CANCELED_SHARDS=0
                        STARTED_AT="$(date -u +%Y-%m-%dT%H:%M:%SZ)"
                        START_EPOCH="$(date +%s)"

                        ${plainShellLogging { }}
                        ${shellCommon}

                        usage() {
                          cat <<'EOF'
        Usage: nix run .#framework::test [-- --profile <feature-proof|ci|full>] [--summary] [--summary-json <path>] [--shard <name>] [--max-parallel-shards <n|auto>] [--serial] [--list-shards]

        Profiles:
          feature-proof Run only direct feature proofs.
          ci            Run canonical feature proofs plus compile, manifest, kernel, adapters, and migration shards.
          full          Run every registered framework check.

        Shards:
          compile     Build compile-time model, help, schema, documentation, and governance proofs.
          manifest    Build manifest-owned feature and fixture contracts.
          kernel      Build kernel-owned runtime semantics, registry, and workflow proofs.
          adapters    Build thin launcher, shell, and process-edge adapter proofs.
          e2e         Build end-to-end public behavior, install, upgrade, isolation, and runtime smokes.
          migration   Build low-scope legacy-regression and ownership-migration checks.
        EOF
                        }

                        print_shards() {
                          local shard_name
                          for shard_name in "''${SHARDS[@]}"; do
                            printf '%s\n' "$shard_name"
                          done
                        }

                        shard_exists() {
                          local candidate="$1"
                          local shard_name
                          for shard_name in "''${SHARDS[@]}"; do
                            if [ "$candidate" = "$shard_name" ]; then
                              return 0
                            fi
                          done
                          return 1
                        }

                        load_profile_shards() {
                          case "$PROFILE" in
                            feature-proof)
                              profile_shards=("''${PROFILE_FEATURE_PROOF_SHARDS[@]}")
                              ;;
                            ci)
                              profile_shards=("''${PROFILE_CI_SHARDS[@]}")
                              ;;
                            full)
                              profile_shards=("''${PROFILE_FULL_SHARDS[@]}")
                              ;;
                            *)
                              log_error "unknown profile '$PROFILE' (expected: feature-proof|ci|full)"
                              exit "$NIXFIED_EXIT_USAGE"
                              ;;
                          esac
                        }

                        write_summary_json() {
                          local rc="$1"
                          local finished_at duration
                          local summary_dir summary_tmp
                          finished_at="$(date -u +%Y-%m-%dT%H:%M:%SZ)"
                          duration="$(( $(date +%s) - START_EPOCH ))"
                          summary_dir="$(dirname "$SUMMARY_JSON")"
                          mkdir -p "$summary_dir"
                          summary_tmp="$(mktemp "$SUMMARY_JSON.tmp.XXXXXX")"
                          cat > "$summary_tmp" <<JSON
        {
          "profile": "$PROFILE",
          "shard": $(if [ -n "$SHARD" ]; then printf '"%s"' "$SHARD"; else printf 'null'; fi),
          "executed_shards": $EXECUTED,
          "failed_shards": $FAILED_SHARDS,
          "exit_1_shards": $EXIT_1_SHARDS,
          "canceled_shards": $CANCELED_SHARDS,
          "exit_code": $rc,
          "duration_seconds": $duration,
          "started_at": "$STARTED_AT",
          "finished_at": "$finished_at"
        }
        JSON
                          mv "$summary_tmp" "$SUMMARY_JSON"
                          log_info "wrote summary json path=$SUMMARY_JSON"
                        }

                        run_shard() {
                          local shard_name="$1"
                          shift
                          log_info "running shard=$shard_name"
                          if [ -n "''${NIXFIED_FRAMEWORK_TEST_FORCE_FAIL_SHARD:-}" ] && [ "$shard_name" = "$NIXFIED_FRAMEWORK_TEST_FORCE_FAIL_SHARD" ]; then
                            log_error "shard failed name=$shard_name rc=17"
                            return 17
                          fi
                          if "$@"; then
                            log_ok "shard passed name=$shard_name"
                            return 0
                          else
                            local rc=$?
                            log_error "shard failed name=$shard_name rc=$rc"
                            return "$rc"
                          fi
                        }

                        run_nix_build_shard() {
                          local -a build_args
                          build_args=(build --no-link)
                          build_args+=("''${@}")
                          nix "''${build_args[@]}"
                        }

                        shard_compile() {
                          run_nix_build_shard \
                ${renderCheckArgs "compile"}
                        }

                        shard_manifest() {
                          run_nix_build_shard \
                ${renderCheckArgs "manifest"}
                        }

                        shard_kernel() {
                          run_nix_build_shard \
                ${renderCheckArgs "kernel"}
                        }

                        shard_adapters() {
                          run_nix_build_shard \
                ${renderCheckArgs "adapters"}
                        }

                        shard_e2e() {
                          run_nix_build_shard \
                ${renderCheckArgs "e2e"}
                          nix run .#run-workflow -- workflow.test.framework.selfhost --summary
                        }

                        shard_migration() {
                          run_nix_build_shard \
                ${renderCheckArgs "migration"}
                        }

                        run_full_shard() {
                          local shard_name="$1"
                          case "$shard_name" in
                            compile)
                              shard_compile
                              ;;
                            manifest)
                              shard_manifest
                              ;;
                            kernel)
                              shard_kernel
                              ;;
                            adapters)
                              shard_adapters
                              ;;
                            e2e)
                              shard_e2e
                              ;;
                            migration)
                              shard_migration
                              ;;
                            *)
                              log_error "unknown shard '$shard_name'"
                              return "$NIXFIED_EXIT_USAGE"
                              ;;
                          esac
                        }

                        run_profile_shard() {
                          local profile_name="$1"
                          local shard_name="$2"
                          case "$profile_name:$shard_name" in
        ${
          lib.concatMapStrings (
            profileName:
            lib.concatMapStrings (
              shardName:
              let
                body = renderProfileShardBody profileName shardName;
              in
              "                            ${lib.escapeShellArg "${profileName}:${shardName}"})\n${body}\n                              ;;\n"
            ) frameworkTestShardNames
          ) frameworkTestProfileNames
        }                            *)
                              log_error "unknown profile/shard selection profile=$profile_name shard=$shard_name"
                              return "$NIXFIED_EXIT_USAGE"
                              ;;
                          esac
                        }

                        run_named_shard() {
                          local shard_name="$1"
                          if [ -n "$SHARD" ]; then
                            run_shard "$shard_name" run_full_shard "$shard_name"
                          else
                            run_shard "$shard_name" run_profile_shard "$PROFILE" "$shard_name"
                          fi
                        }

                        run_named_shard_recorded() {
                          local shard_name="$1"
                          local rc=0
                          if run_named_shard "$shard_name"; then
                            EXECUTED="$((EXECUTED + 1))"
                            return 0
                          else
                            rc="$?"
                            FAILED_SHARDS="$((FAILED_SHARDS + 1))"
                            if [ "$rc" -eq 1 ]; then
                              EXIT_1_SHARDS="$((EXIT_1_SHARDS + 1))"
                            fi
                          fi
                          return "$rc"
                        }

                        resolve_parallel_workers() {
                          local requested="$1"
                          local shard_total="$2"
                          local workers="$shard_total"

                          if [ "$requested" != "auto" ]; then
                            workers="$requested"
                          fi

                          if [ "$workers" -gt "$shard_total" ]; then
                            workers="$shard_total"
                          fi

                          if [ "$workers" -lt 1 ]; then
                            workers=1
                          fi

                          printf '%s' "$workers"
                        }

                        run_shards_parallel() {
                          local requested_workers="$1"
                          shift
                          local shard_names=("''${@}")
                          local shard_total="''${#shard_names[@]}"
                          local workers
                          local next_index=0
                          local running_count=0
                          local done_pid=""
                          local done_shard=""
                          local wait_rc=0
                          local pid
                          local failed=0
                          local first_rc=1
                          local failed_shard=""
                          local pending_canceled=0
                          local -A PID_TO_SHARD=()
                          local -A CANCEL_REQUESTED=()

                          if [ "$shard_total" -eq 0 ]; then
                            return 0
                          fi

                          workers="$(resolve_parallel_workers "$requested_workers" "$shard_total")"
                          if [ "$workers" -le 1 ]; then
                            for shard_name in "''${shard_names[@]}"; do
                              run_named_shard_recorded "$shard_name" || return $?
                            done
                            return 0
                          fi

                          log_info "running shards parallel workers=$workers total=$shard_total"

                          start_shard_worker() {
                            local shard_name="$1"
                            (
                              set +e
                              run_named_shard "$shard_name"
                            ) &
                            pid="$!"
                            PID_TO_SHARD[$pid]="$shard_name"
                            CANCEL_REQUESTED[$pid]=0
                            running_count="$((running_count + 1))"
                          }

                          cancel_running_shards() {
                            local active_pid
                            for active_pid in "''${!PID_TO_SHARD[@]}"; do
                              CANCEL_REQUESTED[$active_pid]=1
                              kill -TERM "$active_pid" 2>/dev/null || true
                            done

                            sleep "$NIXFIED_RETRY_INTERVAL_DEFAULT"
                            for active_pid in "''${!PID_TO_SHARD[@]}"; do
                              if kill -0 "$active_pid" 2>/dev/null; then
                                kill -KILL "$active_pid" 2>/dev/null || true
                              fi
                            done
                          }

                          while [ "$running_count" -lt "$workers" ] && [ "$next_index" -lt "$shard_total" ]; do
                            start_shard_worker "''${shard_names[$next_index]}"
                            next_index="$((next_index + 1))"
                          done

                          while [ "''${#PID_TO_SHARD[@]}" -gt 0 ]; do
                            if wait -n -p done_pid; then
                              wait_rc=0
                            else
                              wait_rc="$?"
                            fi

                            done_shard="''${PID_TO_SHARD[$done_pid]:-}"
                            if [ -z "$done_shard" ]; then
                              continue
                            fi

                            unset "PID_TO_SHARD[$done_pid]"
                            running_count="$((running_count - 1))"

                            if [ "''${CANCEL_REQUESTED[$done_pid]:-0}" = "1" ]; then
                              CANCELED_SHARDS="$((CANCELED_SHARDS + 1))"
                              continue
                            fi

                            if [ "$wait_rc" -eq 0 ]; then
                              EXECUTED="$((EXECUTED + 1))"
                            else
                              FAILED_SHARDS="$((FAILED_SHARDS + 1))"
                              if [ "$wait_rc" -eq 1 ]; then
                                EXIT_1_SHARDS="$((EXIT_1_SHARDS + 1))"
                              fi
                              if [ "$failed" -eq 0 ]; then
                                first_rc="$wait_rc"
                                failed_shard="$done_shard"
                                pending_canceled="$((shard_total - next_index))"
                                CANCELED_SHARDS="$((CANCELED_SHARDS + pending_canceled))"
                                log_warn "framework::test fail-fast shard=$failed_shard rc=$first_rc pending_canceled=$pending_canceled running_canceled=''${#PID_TO_SHARD[@]}"
                                cancel_running_shards
                              fi
                              failed=1
                            fi

                            if [ "$failed" -eq 0 ]; then
                              while [ "$running_count" -lt "$workers" ] && [ "$next_index" -lt "$shard_total" ]; do
                                start_shard_worker "''${shard_names[$next_index]}"
                                next_index="$((next_index + 1))"
                              done
                            fi
                          done

                          if [ "$failed" -eq 1 ]; then
                            return "$first_rc"
                          fi
                          return 0
                        }

                        while [ "$#" -gt 0 ]; do
                          case "$1" in
                            --profile)
                              PROFILE="$(nixfied_require_next_arg --profile "a value" "$@")"
                              shift 2
                              ;;
                            --summary)
                              SUMMARY=1
                              shift
                              ;;
                            --summary-json)
                              SUMMARY_JSON="$(nixfied_require_next_arg --summary-json "a value" "$@")"
                              shift 2
                              ;;
                            --shard)
                              SHARD="$(nixfied_require_next_arg --shard "a value" "$@")"
                              shift 2
                              ;;
                            --max-parallel-shards)
                              MAX_PARALLEL_SHARDS="$(nixfied_require_next_arg --max-parallel-shards "a value" "$@")"
                              shift 2
                              ;;
                            --serial)
                              SERIAL=1
                              shift
                              ;;
                            --list-shards)
                              LIST_SHARDS=1
                              shift
                              ;;
                            --help|-h)
                              usage
                              exit 0
                              ;;
                            --)
                              shift
                              break
                              ;;
                            *)
                              nixfied_unknown_arg_with_usage usage "$1"
                              ;;
                          esac
                        done

                        nixfied_unexpected_positional_args_with_usage usage "$@"

                        load_profile_shards

                        case "$MAX_PARALLEL_SHARDS" in
                          auto)
                            ;;
                          *)
                            if ! [[ "$MAX_PARALLEL_SHARDS" =~ ^[0-9]+$ ]]; then
                              log_error "invalid --max-parallel-shards '$MAX_PARALLEL_SHARDS' (expected: auto|positive-integer)"
                              exit "$NIXFIED_EXIT_USAGE"
                            fi
                            if [ "$MAX_PARALLEL_SHARDS" -lt 1 ]; then
                              log_error "invalid --max-parallel-shards '$MAX_PARALLEL_SHARDS' (expected: auto|positive-integer)"
                              exit "$NIXFIED_EXIT_USAGE"
                            fi
                            ;;
                        esac

                        if [ "$LIST_SHARDS" -eq 1 ]; then
                          print_shards
                          exit 0
                        fi

                        if [ -n "$SHARD" ] && ! shard_exists "$SHARD"; then
                          log_error "unknown shard '$SHARD'"
                          log_info "valid shards: $(print_shards | tr '\n' ' ')"
                          exit "$NIXFIED_EXIT_USAGE"
                        fi

                        cleanup() {
                          local rc=$?
                          if [ -n "$SUMMARY_JSON" ]; then
                            write_summary_json "$rc"
                          fi
                          return "$rc"
                        }
                        trap cleanup EXIT

                        selected_shards=()
                        if [ -n "$SHARD" ]; then
                          selected_shards+=("$SHARD")
                        else
                          selected_shards=("''${profile_shards[@]}")
                        fi

                        run_rc=0
                        if [ "$SERIAL" -eq 1 ]; then
                          log_info "running shards serial total=''${#selected_shards[@]}"
                          for shard_name in "''${selected_shards[@]}"; do
                            if run_named_shard_recorded "$shard_name"; then
                              :
                            else
                              run_rc="$?"
                              break
                            fi
                          done
                        elif [ "''${#selected_shards[@]}" -le 1 ]; then
                          for shard_name in "''${selected_shards[@]}"; do
                            if run_named_shard_recorded "$shard_name"; then
                              :
                            else
                              run_rc="$?"
                              break
                            fi
                          done
                        else
                          if run_shards_parallel "$MAX_PARALLEL_SHARDS" "''${selected_shards[@]}"; then
                            run_rc=0
                          else
                            run_rc="$?"
                          fi
                        fi

                        if [ "$SUMMARY" -eq 1 ] || [ "$run_rc" -ne 0 ]; then
                          log_info "summary profile=$PROFILE executed_shards=$EXECUTED failed_shards=$FAILED_SHARDS exit_1_shards=$EXIT_1_SHARDS canceled_shards=$CANCELED_SHARDS"
                        fi

                        if [ "$run_rc" -ne 0 ]; then
                          exit "$run_rc"
                        fi

                        log_ok "framework::test completed"
      '';
      launcher = mkTaskLauncher {
        appId = "framework::test";
        category = "framework";
        usage = [
          "nix run .#framework::test"
          "nix run .#framework::test -- --profile feature-proof --summary"
          "nix run .#framework::test -- --profile full --summary"
          "nix run .#framework::test -- --profile ci --summary-json /tmp/framework-summary.json"
        ];
        examples = [
          "nix run .#framework::test -- --list-shards"
          "nix run .#framework::test -- --shard compile"
          "nix run .#framework::test -- --shard kernel"
          "nix run .#framework::test -- --shard e2e"
          "nix run .#framework::test -- --shard migration"
        ];
        inherit ownerFile;
      };
      inherit ownerFile;
    };
  };
}
