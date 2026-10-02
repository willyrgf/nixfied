# Gate-runtime: the framework's own runtime-layer test suite expressed as a
# first-class nixfied manifest. Every test here exercises the Rust runtime as an
# adopter would — parallel examples, sequential lifecycle, concurrent slot
# isolation, and structured assertion steps — with no bespoke orchestration.
#
# Closure `rt` and the manifest paths are injected by the flake.nix override
# module. The pure-nixpkgs closures below are complete on their own.
{ pkgs, nixfiedLib, ... }:
{
  nixfied.project.projectId = "gate-runtime";
  nixfied.project.name = "Gate Runtime Tests";
  nixfied.codebases.main.logicalRoot = ".";

  # ---- closures -------------------------------------------------------
  # rt is declared in the flake.nix compileManifest override.
  nixfied.closures.jq = {
    package = pkgs.jq;
    executable = "bin/jq";
  };
  nixfied.closures.coreutils = {
    package = pkgs.coreutils;
    executable = "bin/mkdir";
    effects = [
      "process"
      "file-write"
    ];
  };
  nixfied.closures.findutils = {
    package = pkgs.findutils;
    executable = "bin/find";
  };
  nixfied.closures.diffutils = {
    package = pkgs.diffutils;
    executable = "bin/cmp";
  };

  # ---- example leaf tasks (run → manifest/docs proof → clean) ------------

  nixfied.tasks.example-minimal = {
    invocation = {
      tools = [
        pkgs.bash
        "rt"
        "jq"
        "coreutils"
      ];
      run = [
        "bash"
        "-c"
        ''
          set -euo pipefail
          mkdir -p "''${stateDir}/gate-artifacts" "''${stateDir}/example-minimal-inner"
          NIXFIED_STATE_DIR="''${stateDir}/example-minimal-inner" \
            nixfied-runtime run --manifest "$MINIMAL_MANIFEST/manifest.json" --task smoke --timeout-ms 60000 --output json \
            > "''${stateDir}/gate-artifacts/example-minimal.json"
          result="''${stateDir}/gate-artifacts/example-minimal.json"
          jq -e '.task.success and .task.exitCode == 0 and
            [.services[].serviceId] == ["synthetic"] and
            [.nodes[].nodeId] == ["smoke"]' "$result" >/dev/null
          [ "$(cat "$(jq -r .task.stdoutPath "$result")")" = ok ]
          jq -e --slurpfile result "$result" '
            .success and .runId == $result[0].runId and
            .tasks == $result[0].tasks and .nodes == $result[0].nodes
          ' "$(jq -r .runSummaryPath "$result")" >/dev/null
          jq -e '.project.projectId == "minimal" and (has("docs") | not)' \
            "$MINIMAL_MANIFEST/manifest.json" >/dev/null
          test -f "$MINIMAL_MANIFEST/views/docs.md"
          test ! -e "$MINIMAL_MANIFEST/views/schema.json"
          test ! -e "$MINIMAL_MANIFEST/views/capabilities.json"
          docs=$(<"$MINIMAL_MANIFEST/views/docs.md")
          [[ "$docs" == *'# Minimal'* ]]
          [[ "$docs" == *'- id: `minimal`'* ]]
          [[ "$docs" == *'### `smoke`'* ]]
          [[ "$docs" == *'- default output: `summary`'* ]]
          [[ "$docs" == *'- services required: `synthetic`'* ]]
          [[ "$docs" == *'### `synthetic`'* ]]
          [[ "$docs" == *'- primary endpoint: `synthetic-tcp`'* ]]
          [[ "$docs" != *'## Surfaces'* ]]
          # Run-scoped application data is deleted by session finalization;
          # retained evidence and cleanup history survive it.
          [ ! -e "''${stateDir}/example-minimal-inner/data/minimal/dev/0" ] \
            || { echo "example-minimal: run-scoped state survived its session" >&2; exit 1; }
          test -f "$(jq -r .task.stdoutPath "$result")"
          clean=$(NIXFIED_STATE_DIR="''${stateDir}/example-minimal-inner" \
            nixfied-runtime clean --manifest "$MINIMAL_MANIFEST/manifest.json")
          jq -e '.result == "absent"' <<<"$clean" >/dev/null
        ''
      ];
    };
  };

  nixfied.tasks.example-postgres = {
    invocation = {
      tools = [
        pkgs.bash
        "rt"
        "jq"
        "coreutils"
      ];
      run = [
        "bash"
        "-c"
        ''
          set -euo pipefail
          mkdir -p "''${stateDir}/gate-artifacts" "''${stateDir}/example-postgres-inner"
          NIXFIED_STATE_DIR="''${stateDir}/example-postgres-inner" \
            nixfied-runtime run --manifest "$POSTGRES_MANIFEST/manifest.json" --task smoke-query --timeout-ms 60000 --output json \
            > "''${stateDir}/gate-artifacts/example-postgres.json"
          result="''${stateDir}/gate-artifacts/example-postgres.json"
          [ "$(cat "$(jq -r .task.stdoutPath "$result")")" = 1 ]
          root="''${stateDir}/example-postgres-inner/data/postgres-example/dev/0"
          # A second invocation must adopt the initialized cluster, run the query,
          # and preserve user state rather than initializing over it.
          touch "$root/pgdata/adoption-sentinel"
          NIXFIED_STATE_DIR="''${stateDir}/example-postgres-inner" \
            nixfied-runtime run --manifest "$POSTGRES_MANIFEST/manifest.json" --task smoke-query --timeout-ms 60000 --output task-output \
            > "''${stateDir}/gate-artifacts/example-postgres-repeat.stdout"
          [ "$(cat "''${stateDir}/gate-artifacts/example-postgres-repeat.stdout")" = 1 ]
          test -f "$root/pgdata/adoption-sentinel"
          if NIXFIED_STATE_DIR="''${stateDir}/example-postgres-inner" \
            nixfied-runtime clean --manifest "$POSTGRES_MANIFEST/manifest.json" 2>/dev/null; then
            echo "example-postgres: ordinary clean deleted persistent data" >&2
            exit 1
          fi
          test -f "$root/pgdata/adoption-sentinel"
          NIXFIED_STATE_DIR="''${stateDir}/example-postgres-inner" \
            nixfied-runtime clean --manifest "$POSTGRES_MANIFEST/manifest.json" --purge
          test ! -e "$root"
        ''
      ];
    };
  };

  nixfied.tasks.example-composite = {
    invocation = {
      tools = [
        pkgs.bash
        "rt"
        "jq"
        "coreutils"
      ];
      run = [
        "bash"
        "-c"
        ''
          set -euo pipefail
          mkdir -p "''${stateDir}/gate-artifacts" "''${stateDir}/example-composite-inner"
          NIXFIED_STATE_DIR="''${stateDir}/example-composite-inner" \
            nixfied-runtime run --manifest "$COMPOSITE_MANIFEST/manifest.json" --task pipeline --timeout-ms 60000 --output json \
            > "''${stateDir}/gate-artifacts/example-composite.json"
          result="''${stateDir}/gate-artifacts/example-composite.json"
          jq -e '
            [.tasks[].stepPath] == ["pipeline.probe", "pipeline.verify"] and
            [.tasks[].taskId] == ["smoke", "smoke"] and
            (all(.tasks[]; .success and .exitCode == 0)) and
            ([.tasks[].processKey] | unique | length) == 2 and
            ([.tasks[].stdoutPath] | unique | length) == 2 and
            ([.tasks[].summaryPath] | unique | length) == 2 and
            [.services[].serviceId] == ["synthetic"]
          ' "$result" >/dev/null
          while IFS= read -r output; do
            [ "$(cat "$output")" = ok ]
          done < <(jq -r '.tasks[].stdoutPath' "$result")
          jq -e --slurpfile result "$result" '
            .success and .tasks == $result[0].tasks and .nodes == $result[0].nodes
          ' "$(jq -r .runSummaryPath "$result")" >/dev/null
          docs=$(<"$COMPOSITE_MANIFEST/views/docs.md")
          [[ "$docs" == *'### `pipeline`'* ]]
          [[ "$docs" == *'- kind: `composite`'* ]]
          [[ "$docs" == *'- default output: `summary`'* ]]
          [[ "$docs" == *'`probe`: task `smoke`; depends on none'* ]]
          [[ "$docs" == *'`verify`: task `smoke`; depends on `probe`'* ]]
          NIXFIED_STATE_DIR="''${stateDir}/example-composite-inner" \
            nixfied-runtime clean --manifest "$COMPOSITE_MANIFEST/manifest.json"
        ''
      ];
    };
  };

  nixfied.tasks.example-downstream = {
    invocation = {
      tools = [
        pkgs.bash
        "rt"
        "coreutils"
      ];
      run = [
        "bash"
        "-c"
        ''
          set -euo pipefail
          mkdir -p "''${stateDir}/gate-artifacts" "''${stateDir}/example-downstream-inner"
          NIXFIED_STATE_DIR="''${stateDir}/example-downstream-inner" \
            nixfied-runtime run --manifest "$DOWNSTREAM_MANIFEST/manifest.json" --task release --timeout-ms 60000 --output json \
            > "''${stateDir}/gate-artifacts/example-downstream.json"
          NIXFIED_STATE_DIR="''${stateDir}/example-downstream-inner" \
            nixfied-runtime clean --manifest "$DOWNSTREAM_MANIFEST/manifest.json"
        ''
      ];
    };
  };

  # Each Reth phase permits 5 * 3 * 2000ms + 4 * 500ms = 32s.
  # Allow both phases plus startup, task and settlement under the outer deadline.
  nixfied.tasks.example-reth = {
    invocation = {
      tools = [
        pkgs.bash
        pkgs.sqlite
        "jq"
        "rt"
        "coreutils"
      ];
      run = [
        "bash"
        "-c"
        ''
          set -euo pipefail
          mkdir -p "''${stateDir}/gate-artifacts" "''${stateDir}/example-reth-inner"
          started=$(date +%s)
          NIXFIED_STATE_DIR="''${stateDir}/example-reth-inner" \
            nixfied-runtime run --manifest "$RETH_MANIFEST/manifest.json" --task reth-smoke --timeout-ms 120000 --output json \
            > "''${stateDir}/gate-artifacts/example-reth.json"
          cold_seconds=$(( $(date +%s) - started ))
          failed_manifest="''${stateDir}/gate-artifacts/reth-failed.json"
          failed_error="''${stateDir}/gate-artifacts/reth-failed-error.json"
          jq '.services.reth.endpoints."reth-ws".readyProbe.run[1] = "invalid-test-mode"' \
            "$RETH_MANIFEST/manifest.json" > "$failed_manifest"
          started=$(date +%s)
          code=0
          NIXFIED_STATE_DIR="''${stateDir}/example-reth-failed" \
            nixfied-runtime run --allow-non-store-manifest --manifest "$failed_manifest" \
              --task reth-smoke --timeout-ms 60000 --output json > /dev/null 2> "$failed_error" || code=$?
          failed_seconds=$(( $(date +%s) - started ))
          [ "$code" -eq 26 ]
          jq -e '.code == "READINESS_TIMEOUT" and .details.lastRound ==
            {"phase":"ready","reason":"probe-failed","endpointId":"reth-ws"}' "$failed_error" > /dev/null
          registry=$(jq -r '.details.registryPath' "$failed_error")
          [ "$(sqlite3 "$registry" "SELECT count(*) FROM events WHERE event_type='endpoint.check-succeeded';")" -eq 0 ]
          [ "$(sqlite3 "$registry" "SELECT count(*) FROM processes WHERE role='task';")" -eq 0 ]
          failed_probes=$(sqlite3 "$registry" "SELECT count(*) FROM processes WHERE role='probe' AND exit_code != 0;")
          # Listener-missing startup rounds consume the same phase budget.
          [ "$failed_probes" -ge 1 ] && [ "$failed_probes" -le 5 ]
          # 32s command/retry ceiling plus 10s stop and 3s overhead margin.
          [ "$failed_seconds" -le 45 ]
          jq -n --argjson coldSeconds "$cold_seconds" --argjson failedSeconds "$failed_seconds" \
            '{coldSeconds:$coldSeconds,failedSeconds:$failedSeconds,phaseCeilingMs:32000,parentTimeoutMs:120000}' \
            | tee "''${stateDir}/gate-artifacts/reth-calibration.json"
          NIXFIED_STATE_DIR="''${stateDir}/example-reth-inner" \
            nixfied-runtime clean --manifest "$RETH_MANIFEST/manifest.json"
        ''
      ];
    };
  };

  nixfied.tasks.example-toolchain = {
    invocation = {
      tools = [
        pkgs.bash
        "rt"
        "coreutils"
        "jq"
      ];
      run = [
        "bash"
        "-c"
        ''
          set -euo pipefail
          mkdir -p "''${stateDir}/gate-artifacts" "''${stateDir}/example-toolchain-inner"
          inner_error="''${stateDir}/gate-artifacts/example-toolchain.error.json"
          if NIXFIED_STATE_DIR="''${stateDir}/example-toolchain-inner" \
            nixfied-runtime run --manifest "$TOOLCHAIN_MANIFEST/manifest.json" --task ci --timeout-ms 60000 --output json \
            > "''${stateDir}/gate-artifacts/example-toolchain.json" 2> "$inner_error"; then
            :
          else
            code=$?
            cat "$inner_error" >&2
            logs_dir=$(jq -r '.details.logsDir // empty' "$inner_error") || logs_dir=""
            if [ -n "$logs_dir" ] && [ -f "$logs_dir/service.postgres.stderr.log" ]; then
              echo "example-toolchain postgres stderr:" >&2
              cat "$logs_dir/service.postgres.stderr.log" >&2
            fi
            exit "$code"
          fi
          NIXFIED_STATE_DIR="''${stateDir}/example-toolchain-inner" \
            nixfied-runtime clean --manifest "$TOOLCHAIN_MANIFEST/manifest.json"
        ''
      ];
    };
  };

  # ---- task-output projection ----------------------------------------

  nixfied.tasks.task-output = {
    invocation = {
      tools = [
        pkgs.bash
        "rt"
        "coreutils"
        "diffutils"
      ];
      run = [
        "bash"
        "-c"
        ''
          set -euo pipefail
          inner="''${stateDir}/task-output-inner"
          artifacts="''${stateDir}/gate-artifacts/task-output"
          mkdir -p "$artifacts"

          assert_stdout() {
            local expected=$1
            local actual=$2
            local expected_path=$3
            printf '%b' "$expected" > "$expected_path"
            cmp -s "$expected_path" "$actual"
          }

          assert_stderr_contains() {
            local actual=$1
            local expected=$2
            [[ "$(<"$actual")" == *"$expected"* ]]
          }

          NIXFIED_STATE_DIR="$inner/success" \
            nixfied-runtime run --manifest "$TASK_OUTPUT_MANIFEST/manifest.json" \
              --task output --output task-output \
              >"$artifacts/success.stdout" 2>"$artifacts/success.stderr"
          assert_stdout '\x00\x01\x02\x00\xff\n' \
            "$artifacts/success.stdout" "$artifacts/success.expected"
          assert_stderr_contains "$artifacts/success.stderr" "diagnostic"

          NIXFIED_STATE_DIR="$inner/accepted" \
            nixfied-runtime run --manifest "$TASK_OUTPUT_MANIFEST/manifest.json" \
              --task accepted --output task-output \
              >"$artifacts/accepted.stdout" 2>"$artifacts/accepted.stderr"
          assert_stdout 'accepted' \
            "$artifacts/accepted.stdout" "$artifacts/accepted.expected"
          assert_stderr_contains "$artifacts/accepted.stderr" "nonzero"

          NIXFIED_STATE_DIR="$inner/redacted" \
            nixfied-runtime run --manifest "$TASK_OUTPUT_MANIFEST/manifest.json" \
              --task redacted --output task-output \
              >"$artifacts/redacted.stdout" 2>"$artifacts/redacted.stderr"
          assert_stdout '[REDACTED]' \
            "$artifacts/redacted.stdout" "$artifacts/redacted.expected"
          ! [[ "$(<"$artifacts/redacted.stdout")" == *"$NIXFIED_TASK_OUTPUT_SECRET"* ]]
          ! [[ "$(<"$artifacts/redacted.stderr")" == *"$NIXFIED_TASK_OUTPUT_SECRET"* ]]

          timeout_code=0
          if NIXFIED_STATE_DIR="$inner/timeout" \
             nixfied-runtime run --manifest "$TASK_OUTPUT_MANIFEST/manifest.json" \
               --task timeout --output task-output \
               >"$artifacts/timeout.stdout" 2>"$artifacts/timeout.stderr"; then
            :
          else
            timeout_code=$?
          fi
          [ "$timeout_code" -eq 30 ]
          assert_stdout 'timeout-output' \
            "$artifacts/timeout.stdout" "$artifacts/timeout.expected"
          assert_stderr_contains "$artifacts/timeout.stderr" "timeout-error"
          assert_stderr_contains "$artifacts/timeout.stderr" "timed out"

          NIXFIED_STATE_DIR="$inner/cancel" \
            nixfied-runtime run --manifest "$TASK_OUTPUT_MANIFEST/manifest.json" \
              --task cancel --output task-output \
              >"$artifacts/cancel.stdout" 2>"$artifacts/cancel.stderr" &
          cancel_pid=$!
          sleep 1
          kill -TERM "$cancel_pid"
          cancel_code=0
          if wait "$cancel_pid"; then
            :
          else
            cancel_code=$?
          fi
          [ "$cancel_code" -eq 27 ]
          assert_stdout 'cancel-output' \
            "$artifacts/cancel.stdout" "$artifacts/cancel.expected"
          assert_stderr_contains "$artifacts/cancel.stderr" "cancel-error"
          assert_stderr_contains "$artifacts/cancel.stderr" "CANCELED"

          composite_state="$inner/composite"
          composite_code=0
          if NIXFIED_STATE_DIR="$composite_state" \
             nixfied-runtime run --manifest "$TASK_OUTPUT_MANIFEST/manifest.json" \
               --task pipeline --output task-output \
               >"$artifacts/composite.stdout" 2>"$artifacts/composite.stderr"; then
            :
          else
            composite_code=$?
          fi
          [ "$composite_code" -eq 37 ]
          [ ! -e "$composite_state" ]
          [ ! -s "$artifacts/composite.stdout" ]
        ''
      ];
    };
  };

  # ---- negative leaf tasks (runtime-layer fail-closed checks) ----------

  nixfied.tasks.negative-no-selection = {
    invocation = {
      tools = [
        pkgs.bash
        "rt"
        "jq"
        "coreutils"
      ];
      run = [
        "bash"
        "-c"
        ''
          set -euo pipefail
          mkdir -p "''${stateDir}/gate-artifacts" "''${stateDir}/negative-inner"
          if NIXFIED_STATE_DIR="''${stateDir}/negative-inner" \
             nixfied-runtime run --manifest "$MINIMAL_MANIFEST/manifest.json" --output json \
             >/dev/null 2>"''${stateDir}/gate-artifacts/negative-no-selection.json"; then
            echo "runtime accepted a run with no task selection" >&2; exit 1
          fi
          tail -n 1 "''${stateDir}/gate-artifacts/negative-no-selection.json" \
            | jq -e '.details.declaredTasks | index("smoke") != null' >/dev/null
        ''
      ];
    };
  };

  nixfied.tasks.negative-undeclared-task = {
    invocation = {
      tools = [
        pkgs.bash
        "rt"
        "coreutils"
      ];
      run = [
        "bash"
        "-c"
        ''
          set -euo pipefail
          mkdir -p "''${stateDir}/negative-inner"
          if NIXFIED_STATE_DIR="''${stateDir}/negative-inner" \
             nixfied-runtime run --manifest "$MINIMAL_MANIFEST/manifest.json" --task does-not-exist \
             >/dev/null 2>/dev/null; then
            echo "runtime accepted an undeclared task" >&2; exit 1
          fi
        ''
      ];
    };
  };

  nixfied.tasks.negative-failure-identity = {
    invocation = {
      tools = [
        pkgs.bash
        "rt"
        "jq"
        "coreutils"
      ];
      run = [
        "bash"
        "-c"
        ''
          set -euo pipefail
          mkdir -p "''${stateDir}/gate-artifacts" "''${stateDir}/negative-inner/identity"
          if NIXFIED_STATE_DIR="''${stateDir}/negative-inner/identity" \
             nixfied-runtime run --manifest "$NEGATIVE_FAIL_MANIFEST/manifest.json" --task failing --output json \
             >/dev/null 2>"''${stateDir}/gate-artifacts/negative-fail.json"; then
            echo "failing composite reported success" >&2; exit 1
          fi
          run_summary=$(
            tail -n 1 "''${stateDir}/gate-artifacts/negative-fail.json" \
              | jq -r '.details.runSummaryPath'
          )
          jq -e '.durationMs >= 0' "$run_summary" >/dev/null
          tail -n 1 "''${stateDir}/gate-artifacts/negative-fail.json" \
            | jq -e '.details.runId and .details.stateRoot and .details.logsDir' >/dev/null
        ''
      ];
    };
  };

  # ---- lifecycle leaf tasks (sequential via the lifecycle composite) ---
  # All tasks share ${stateDir}/lifecycle-inner-state as the inner state dir
  # so that each step sees the evidence left by its predecessor. Hash values
  # are threaded through ${stateDir}/gate-artifacts/lifecycle-*.txt files.

  nixfied.tasks.lifecycle-first-run = {
    invocation = {
      tools = [
        pkgs.bash
        "rt"
        "jq"
        "coreutils"
      ];
      run = [
        "bash"
        "-c"
        ''
          set -euo pipefail
          mkdir -p "''${stateDir}/gate-artifacts" "''${stateDir}/lifecycle-inner-state"
          NIXFIED_STATE_DIR="''${stateDir}/lifecycle-inner-state" \
            nixfied-runtime run --manifest "$MINIMAL_MANIFEST/manifest.json" --task smoke >/dev/null
          root="''${stateDir}/lifecycle-inner-state/data/minimal/dev/0"
          touch "$root/sentinel"
          jq -r .computedManifestHash "$root/.nixfied-state.json" \
            > "''${stateDir}/gate-artifacts/lifecycle-hash1.txt"
        ''
      ];
    };
  };

  nixfied.tasks.lifecycle-second-run = {
    invocation = {
      tools = [
        pkgs.bash
        "rt"
        "jq"
        "coreutils"
      ];
      run = [
        "bash"
        "-c"
        ''
          set -euo pipefail
          root="''${stateDir}/lifecycle-inner-state/data/minimal/dev/0"
          marker="$root/.nixfied-state.json"
          hash1=$(cat "''${stateDir}/gate-artifacts/lifecycle-hash1.txt")
          NIXFIED_STATE_DIR="''${stateDir}/lifecycle-inner-state" \
            nixfied-runtime run --manifest "$MINIMAL_MANIFEST/manifest.json" --task smoke >/dev/null
          [ -e "$root/sentinel" ] \
            || { echo "lifecycle: second run lost the sentinel file" >&2; exit 1; }
          [ "$(jq -r .computedManifestHash "$marker")" = "$hash1" ] \
            || { echo "lifecycle: second run rewrote marker provenance" >&2; exit 1; }
        ''
      ];
    };
  };

  nixfied.tasks.lifecycle-change-preserves-data = {
    invocation = {
      tools = [
        pkgs.bash
        "rt"
        "jq"
        "coreutils"
      ];
      run = [
        "bash"
        "-c"
        ''
          set -euo pipefail
          root="''${stateDir}/lifecycle-inner-state/data/minimal/dev/0"
          marker="$root/.nixfied-state.json"
          hash1=$(cat "''${stateDir}/gate-artifacts/lifecycle-hash1.txt")
          NIXFIED_STATE_DIR="''${stateDir}/lifecycle-inner-state" \
            nixfied-runtime run --manifest "$MINIMAL_B_MANIFEST/manifest.json" --task smoke >/dev/null
          [ -e "$root/sentinel" ] \
            || { echo "lifecycle: configuration change deleted retained application data" >&2; exit 1; }
          [ "$(jq -r .computedManifestHash "$marker")" != "$hash1" ] \
            || { echo "lifecycle: configuration change did not rewrite provenance" >&2; exit 1; }
          [ "$(jq -r .markerVersion "$marker")" = "3" ] \
            || { echo "lifecycle: unexpected state marker version" >&2; exit 1; }
        ''
      ];
    };
  };

  nixfied.tasks.lifecycle-tamper-refusal = {
    invocation = {
      tools = [
        pkgs.bash
        "rt"
        "jq"
        "coreutils"
      ];
      run = [
        "bash"
        "-c"
        ''
          set -euo pipefail
          marker="''${stateDir}/lifecycle-inner-state/data/minimal/dev/0/.nixfied-state.json"
          mkdir -p "''${stateDir}/gate-artifacts"
          jq '.projectId = "intruder"' "$marker" > "$marker.tmp" && mv "$marker.tmp" "$marker"
          code=0
          NIXFIED_STATE_DIR="''${stateDir}/lifecycle-inner-state" \
            nixfied-runtime run --manifest "$MINIMAL_B_MANIFEST/manifest.json" --task smoke \
            >/dev/null 2>"''${stateDir}/gate-artifacts/lifecycle-tamper-owner.json" || code=$?
          [ "$code" -eq 21 ] \
            || { echo "lifecycle: tampered ownership exited $code, want 21 (STATE_UNOWNED)" >&2; exit 1; }
          jq '.projectId = "minimal" | .runtimeAbi = "nixfied-runtime-abi:0-foreign"' \
            "$marker" > "$marker.tmp" && mv "$marker.tmp" "$marker"
          code=0
          NIXFIED_STATE_DIR="''${stateDir}/lifecycle-inner-state" \
            nixfied-runtime run --manifest "$MINIMAL_B_MANIFEST/manifest.json" --task smoke \
            >/dev/null 2>"''${stateDir}/gate-artifacts/lifecycle-tamper-abi.json" || code=$?
          [ "$code" -eq 21 ] \
            || { echo "lifecycle: tampered runtime ABI exited $code, want 21 (STATE_UNOWNED)" >&2; exit 1; }
        ''
      ];
    };
  };

  nixfied.tasks.lifecycle-session-ownership = {
    invocation = {
      tools = [
        pkgs.bash
        "rt"
        "jq"
        "coreutils"
      ];
      run = [
        "bash"
        "-c"
        ''
          set -euo pipefail
          inner="''${stateDir}/session-ownership-inner"
          artifacts="''${stateDir}/gate-artifacts"
          mkdir -p "$artifacts" "$inner"
          for occurrence in first second; do
            NIXFIED_STATE_DIR="$inner" \
              nixfied-runtime run --manifest "$SESSION_ENDPOINT_MANIFEST/manifest.json" \
                --task smoke --timeout-ms 60000 --output json \
              > "$artifacts/session-$occurrence.json"
            NIXFIED_STATE_DIR="$inner" \
              nixfied-runtime ps --manifest "$SESSION_ENDPOINT_MANIFEST/manifest.json" \
              > "$artifacts/session-$occurrence-ps.json"
            jq -e '[.processes[] | select(.live == true)] | length == 0' \
              "$artifacts/session-$occurrence-ps.json" >/dev/null
          done
          first_process=$(jq -r '.services[0].processKey' "$artifacts/session-first.json")
          second_process=$(jq -r '.services[0].processKey' "$artifacts/session-second.json")
          first_instance=$(jq -r '.services[0].serviceInstanceId' "$artifacts/session-first.json")
          second_instance=$(jq -r '.services[0].serviceInstanceId' "$artifacts/session-second.json")
          [ "$first_process" != "$second_process" ] && [ "$first_instance" != "$second_instance" ] \
            || { echo "session ownership: later run reused service process evidence" >&2; exit 1; }
          jq -e '[.processes[] | select(.serviceInstanceId != null and .registryStatus == "stopped")] | length == 2' \
            "$artifacts/session-second-ps.json" >/dev/null
          [ -f "$inner/data/minimal/dev/0/endpoint-prepare-sentinel" ] \
            || { echo "session ownership: retained application data missing" >&2; exit 1; }
        ''
      ];
    };
  };

  nixfied.tasks.lifecycle-purge = {
    invocation = {
      tools = [
        pkgs.bash
        "rt"
        "jq"
        "coreutils"
      ];
      run = [
        "bash"
        "-c"
        ''
          set -euo pipefail
          inner="''${stateDir}/purge-inner"
          mkdir -p "''${stateDir}/gate-artifacts" "$inner"
          NIXFIED_STATE_DIR="$inner" \
            nixfied-runtime run --manifest "$PURGE_MINIMAL_MANIFEST/manifest.json" \
              --task smoke --timeout-ms 60000 \
            > "''${stateDir}/gate-artifacts/purge-run.json"
          if NIXFIED_STATE_DIR="$inner" \
             nixfied-runtime clean --manifest "$PURGE_MINIMAL_MANIFEST/manifest.json" \
             >"''${stateDir}/gate-artifacts/purge-clean-standard.json" \
             2>"''${stateDir}/gate-artifacts/purge-clean-standard.err"; then
            echo "purge: standard clean accepted persistent state" >&2
            exit 1
          fi
          NIXFIED_STATE_DIR="$inner" \
            nixfied-runtime clean --manifest "$PURGE_MINIMAL_MANIFEST/manifest.json" --purge \
            > "''${stateDir}/gate-artifacts/purge-clean.json"
          deleted=$(jq -r '.deletedPath' "''${stateDir}/gate-artifacts/purge-clean.json")
          jq -e '.result == "deleted" and .cleanupId and .deletedPath' \
            "''${stateDir}/gate-artifacts/purge-clean.json" >/dev/null
          [ -n "$deleted" ] && [ "$deleted" != "null" ] \
            || { echo "purge: clean output did not report deletedPath" >&2; exit 1; }
          [ ! -e "$deleted" ] \
            || { echo "purge: deletedPath still exists" >&2; exit 1; }
        ''
      ];
    };
  };

  # ---- host endpoint coordination across independent state roots ------

  nixfied.tasks.endpoint-cross-root = {
    invocation = {
      tools = [
        pkgs.bash
        "rt"
        "jq"
        "coreutils"
        "findutils"
      ];
      run = [
        "bash"
        "-c"
        ''
          set -euo pipefail
          root_a="''${stateDir}/endpoint-root-a"
          root_b="''${stateDir}/endpoint-root-b"
          artifacts="''${stateDir}/gate-artifacts"
          mkdir -p "$root_a" "$root_b" "$artifacts"
          owner_pid=""
          cleanup_endpoint_owner() {
            if [ -n "$owner_pid" ]; then
              kill -TERM "$owner_pid" 2>/dev/null || true
              wait "$owner_pid" 2>/dev/null || true
            fi
          }
          trap cleanup_endpoint_owner EXIT
          rm -f "$root_a/data/minimal/dev/0/session-active"
          NIXFIED_STATE_DIR="$root_a" \
            nixfied-runtime run --manifest "$SESSION_ENDPOINT_MANIFEST/manifest.json" \
              --task keep-up --timeout-ms 60000 --output json \
            > "$artifacts/endpoint-root-a.json" 2> "$artifacts/endpoint-root-a.err" &
          owner_pid=$!
          for attempt in $(seq 1 500); do
            [ ! -f "$root_a/data/minimal/dev/0/session-active" ] || break
            kill -0 "$owner_pid" 2>/dev/null \
              || { cat "$artifacts/endpoint-root-a.err" >&2; exit 1; }
            sleep 0.02
          done
          [ -f "$root_a/data/minimal/dev/0/session-active" ] \
            || { echo "endpoint: root A session never reached its task" >&2; exit 1; }
          [ "$(find "$root_a" -name endpoint-prepare-sentinel -type f | wc -l)" -eq 1 ] \
            || { echo "endpoint: root A prepare sentinel missing" >&2; exit 1; }

          if NIXFIED_STATE_DIR="$root_b" \
             nixfied-runtime run --manifest "$SESSION_ENDPOINT_MANIFEST/manifest.json" \
               --task keep-up --timeout-ms 60000 --output json \
             >/dev/null 2>"$artifacts/endpoint-root-b-conflict.json"; then
            echo "endpoint: independent root B took root A's live listener" >&2
            exit 1
          fi
          tail -n 1 "$artifacts/endpoint-root-b-conflict.json" \
            | jq -e '
                .code == "PORT_CONFLICT"
                and .details.portConflict.reason == "bind-unavailable"
                and .details.portConflict.endpoint.endpointId == "synthetic-tcp"
              ' >/dev/null
          [ "$(find "$root_b" -name endpoint-prepare-sentinel -type f | wc -l)" -eq 0 ] \
            || { echo "endpoint: root B prepared before conflict refusal" >&2; exit 1; }

          kill -TERM "$owner_pid"
          owner_status=0
          wait "$owner_pid" || owner_status=$?
          owner_pid=""
          [ "$owner_status" -eq 27 ] \
            || { echo "endpoint: owner did not finish canceled teardown" >&2; exit 1; }
          NIXFIED_STATE_DIR="$root_b" \
            nixfied-runtime run --manifest "$SESSION_ENDPOINT_MANIFEST/manifest.json" \
               --task smoke --timeout-ms 60000 --output json \
            > "$artifacts/endpoint-root-b.json"
          [ "$(find "$root_b" -name endpoint-prepare-sentinel -type f | wc -l)" -eq 1 ] \
            || { echo "endpoint: root B did not prepare after root A stopped" >&2; exit 1; }
          NIXFIED_STATE_DIR="$root_b" \
            nixfied-runtime ps --manifest "$SESSION_ENDPOINT_MANIFEST/manifest.json" \
            | jq -e '[.processes[] | select(.live == true)] | length == 0' >/dev/null
          trap - EXIT
        ''
      ];
    };
  };

  # ---- slot leaf tasks (slot-0 and slot-1 run concurrently) -----------

  nixfied.tasks.slot-0 = {
    invocation = {
      tools = [
        pkgs.bash
        "rt"
        "coreutils"
      ];
      run = [
        "bash"
        "-c"
        ''
          set -euo pipefail
          mkdir -p "''${stateDir}/gate-artifacts" "''${stateDir}/slots-inner"
          NIXFIED_STATE_DIR="''${stateDir}/slots-inner" \
            nixfied-runtime run --manifest "$DOWNSTREAM_MANIFEST/manifest.json" \
            --task release --slot 0 --timeout-ms 60000 --output json \
            > "''${stateDir}/gate-artifacts/slots-0.json"
        ''
      ];
    };
  };

  nixfied.tasks.slot-1 = {
    invocation = {
      tools = [
        pkgs.bash
        "rt"
        "coreutils"
      ];
      run = [
        "bash"
        "-c"
        ''
          set -euo pipefail
          mkdir -p "''${stateDir}/gate-artifacts" "''${stateDir}/slots-inner"
          NIXFIED_STATE_DIR="''${stateDir}/slots-inner" \
            nixfied-runtime run --manifest "$DOWNSTREAM_MANIFEST/manifest.json" \
            --task release --slot 1 --timeout-ms 60000 --output json \
            > "''${stateDir}/gate-artifacts/slots-1.json"
        ''
      ];
    };
  };

  nixfied.tasks.slots-assert = {
    invocation = {
      tools = [
        pkgs.bash
        "rt"
        "jq"
        "coreutils"
      ];
      run = [
        "bash"
        "-c"
        ''
          set -euo pipefail
          s0="''${stateDir}/gate-artifacts/slots-0.json"
          s1="''${stateDir}/gate-artifacts/slots-1.json"
          disjoint() {
            local inter
            inter=$(jq -n \
              --argjson a "$(jq "$1" "$s0")" \
              --argjson b "$(jq "$1" "$s1")" \
              '[ $a[] | select(. as $x | $b | index($x)) ] | length')
            [ "$inter" -eq 0 ]
          }
          disjoint '[.services[].selectedEndpoint.port]' \
            || { echo "slots: ports overlapped" >&2; exit 1; }
          disjoint '[.services[].serviceInstanceId]' \
            || { echo "slots: shared a serviceInstanceId" >&2; exit 1; }
          disjoint '[.services[].processKey]' \
            || { echo "slots: shared a processKey" >&2; exit 1; }
          NIXFIED_STATE_DIR="''${stateDir}/slots-inner" \
            nixfied-runtime clean --manifest "$DOWNSTREAM_MANIFEST/manifest.json" --slot 0 \
            > "''${stateDir}/gate-artifacts/slots-clean-0.json"
          NIXFIED_STATE_DIR="''${stateDir}/slots-inner" \
            nixfied-runtime clean --manifest "$DOWNSTREAM_MANIFEST/manifest.json" --slot 1 \
            > "''${stateDir}/gate-artifacts/slots-clean-1.json"
          # Each run-scoped session already deleted its own slot data; clean
          # observes per-slot absence without inventing a deletion.
          clean0=$(jq -r 'select(.result == "absent") | .targetPath' "''${stateDir}/gate-artifacts/slots-clean-0.json")
          clean1=$(jq -r 'select(.result == "absent") | .targetPath' "''${stateDir}/gate-artifacts/slots-clean-1.json")
          [ -n "$clean0" ] && [ ! -e "$clean0" ] \
            || { echo "slots: slot 0 run-scoped data survived its session" >&2; exit 1; }
          [ -n "$clean1" ] && [ ! -e "$clean1" ] \
            || { echo "slots: slot 1 run-scoped data survived its session" >&2; exit 1; }
          [ "$clean0" != "$clean1" ] \
            || { echo "slots: clean reported the same state path for both slots" >&2; exit 1; }
        ''
      ];
    };
  };

  # ---- composites ------------------------------------------------------

  nixfied.tasks.examples = {
    kind = "composite";
    steps = {
      example-minimal.task = "example-minimal";
      example-postgres.task = "example-postgres";
      example-composite.task = "example-composite";
      example-downstream.task = "example-downstream";
      example-reth.task = "example-reth";
      example-toolchain.task = "example-toolchain";
    };
  };

  nixfied.tasks.negatives = {
    kind = "composite";
    steps = {
      no-selection.task = "negative-no-selection";
      undeclared-task.task = "negative-undeclared-task";
      failure-identity.task = "negative-failure-identity";
    };
  };

  nixfied.tasks.lifecycle = {
    kind = "composite";
    steps = nixfiedLib.seq [
      "lifecycle-first-run"
      "lifecycle-second-run"
      "lifecycle-change-preserves-data"
      "lifecycle-tamper-refusal"
      "lifecycle-session-ownership"
      "lifecycle-purge"
    ];
  };

  nixfied.tasks.slots = {
    kind = "composite";
    steps = {
      slot-0.task = "slot-0";
      slot-1.task = "slot-1";
      slots-assert = {
        task = "slots-assert";
        dependsOn = [
          "slot-0"
          "slot-1"
        ];
      };
    };
  };

  nixfied.tasks.all = {
    kind = "composite";
    steps = {
      examples.task = "examples";
      negatives = {
        task = "negatives";
        dependsOn = [ "examples" ];
      };
      lifecycle = {
        task = "lifecycle";
        dependsOn = [ "negatives" ];
      };
      task-output = {
        task = "task-output";
        dependsOn = [ "lifecycle" ];
      };
      endpoint = {
        task = "endpoint-cross-root";
        dependsOn = [ "task-output" ];
      };
      slots = {
        task = "slots";
        dependsOn = [ "endpoint" ];
      };
    };
  };
}
