# Gate-runtime: the framework's own runtime-layer test suite expressed as a
# first-class nixfied model. Every test here exercises the Rust runtime as an
# adopter would — parallel examples, sequential lifecycle, concurrent slot
# isolation, and structured assertion steps — with no bespoke orchestration.
#
# Closure `rt` and the model paths are injected by the flake.nix override
# module. The pure-nixpkgs closures below are complete on their own.
{ pkgs, nixfiedLib, ... }:
{
  nixfied.project.projectId = "gate-runtime";
  nixfied.project.name = "Gate Runtime Tests";
  nixfied.codebases.main.logicalRoot = ".";

  # ---- closures -------------------------------------------------------
  # rt is declared in the flake.nix compileModel override.
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

  # ---- example leaf tasks (run → model/docs proof → clean) ------------

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
            nixfied-runtime run --model "$MINIMAL_MODEL/model.json" --task smoke --timeout-ms 60000 --output json \
            > "''${stateDir}/gate-artifacts/example-minimal.json"
          jq -e '.durationMs >= 0 and .task.durationMs >= 0 and .tasks[0].durationMs >= 0 and .nodes[0].durationMs >= 0' \
            "''${stateDir}/gate-artifacts/example-minimal.json" >/dev/null
          jq -e '.durationMs >= 0' \
            "$(jq -r .runSummaryPath "''${stateDir}/gate-artifacts/example-minimal.json")" >/dev/null
          jq -e '.project.projectId == "minimal" and (has("docs") | not)' \
            "$MINIMAL_MODEL/model.json" >/dev/null
          test -f "$MINIMAL_MODEL/views/docs.md"
          test ! -e "$MINIMAL_MODEL/views/schema.json"
          test ! -e "$MINIMAL_MODEL/views/capabilities.json"
          docs=$(<"$MINIMAL_MODEL/views/docs.md")
          [[ "$docs" == *'# Minimal'* ]]
          [[ "$docs" == *'- id: `minimal`'* ]]
          [[ "$docs" == *'### `smoke`'* ]]
          [[ "$docs" == *'- default output: `summary`'* ]]
          [[ "$docs" == *'- services required: `synthetic`'* ]]
          [[ "$docs" == *'### `synthetic`'* ]]
          [[ "$docs" == *'- primary endpoint: `synthetic-tcp`'* ]]
          [[ "$docs" != *'## Surfaces'* ]]
          NIXFIED_STATE_DIR="''${stateDir}/example-minimal-inner" \
            nixfied-runtime clean --model "$MINIMAL_MODEL/model.json"
        ''
      ];
    };
  };

  nixfied.tasks.example-postgres = {
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
          mkdir -p "''${stateDir}/gate-artifacts" "''${stateDir}/example-postgres-inner"
          NIXFIED_STATE_DIR="''${stateDir}/example-postgres-inner" \
            nixfied-runtime run --model "$POSTGRES_MODEL/model.json" --task smoke-query --timeout-ms 60000 --output json \
            > "''${stateDir}/gate-artifacts/example-postgres.json"
          NIXFIED_STATE_DIR="''${stateDir}/example-postgres-inner" \
            nixfied-runtime clean --model "$POSTGRES_MODEL/model.json"
        ''
      ];
    };
  };

  nixfied.tasks.example-composite = {
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
          mkdir -p "''${stateDir}/gate-artifacts" "''${stateDir}/example-composite-inner"
          NIXFIED_STATE_DIR="''${stateDir}/example-composite-inner" \
            nixfied-runtime run --model "$COMPOSITE_MODEL/model.json" --task pipeline --timeout-ms 60000 --output json \
            > "''${stateDir}/gate-artifacts/example-composite.json"
          docs=$(<"$COMPOSITE_MODEL/views/docs.md")
          [[ "$docs" == *'### `pipeline`'* ]]
          [[ "$docs" == *'- kind: `composite`'* ]]
          [[ "$docs" == *'- default output: `summary`'* ]]
          [[ "$docs" == *'`probe`: task `smoke`; depends on none'* ]]
          [[ "$docs" == *'`verify`: task `smoke`; depends on `probe`'* ]]
          NIXFIED_STATE_DIR="''${stateDir}/example-composite-inner" \
            nixfied-runtime clean --model "$COMPOSITE_MODEL/model.json"
        ''
      ];
    };
  };

  nixfied.tasks.example-polyglot = {
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
          mkdir -p "''${stateDir}/gate-artifacts" "''${stateDir}/example-polyglot-inner"
          NIXFIED_STATE_DIR="''${stateDir}/example-polyglot-inner" \
            nixfied-runtime run --model "$POLYGLOT_MODEL/model.json" --task all --timeout-ms 60000 --output json \
            > "''${stateDir}/gate-artifacts/example-polyglot.json"
          NIXFIED_STATE_DIR="''${stateDir}/example-polyglot-inner" \
            nixfied-runtime clean --model "$POLYGLOT_MODEL/model.json"
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
            nixfied-runtime run --model "$DOWNSTREAM_MODEL/model.json" --task release --timeout-ms 60000 --output json \
            > "''${stateDir}/gate-artifacts/example-downstream.json"
          NIXFIED_STATE_DIR="''${stateDir}/example-downstream-inner" \
            nixfied-runtime clean --model "$DOWNSTREAM_MODEL/model.json"
        ''
      ];
    };
  };

  nixfied.tasks.example-reth = {
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
          mkdir -p "''${stateDir}/gate-artifacts" "''${stateDir}/example-reth-inner"
          NIXFIED_STATE_DIR="''${stateDir}/example-reth-inner" \
            nixfied-runtime run --model "$RETH_MODEL/model.json" --task reth-smoke --timeout-ms 60000 --output json \
            > "''${stateDir}/gate-artifacts/example-reth.json"
          NIXFIED_STATE_DIR="''${stateDir}/example-reth-inner" \
            nixfied-runtime clean --model "$RETH_MODEL/model.json"
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
      ];
      run = [
        "bash"
        "-c"
        ''
          set -euo pipefail
          mkdir -p "''${stateDir}/gate-artifacts" "''${stateDir}/example-toolchain-inner"
          NIXFIED_STATE_DIR="''${stateDir}/example-toolchain-inner" \
            nixfied-runtime run --model "$TOOLCHAIN_MODEL/model.json" --task ci --timeout-ms 60000 --output json \
            > "''${stateDir}/gate-artifacts/example-toolchain.json"
          NIXFIED_STATE_DIR="''${stateDir}/example-toolchain-inner" \
            nixfied-runtime clean --model "$TOOLCHAIN_MODEL/model.json"
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
            nixfied-runtime run --model "$TASK_OUTPUT_MODEL/model.json" \
              --task output --output task-output \
              >"$artifacts/success.stdout" 2>"$artifacts/success.stderr"
          assert_stdout '\x00\x01\x02\x00\xff\n' \
            "$artifacts/success.stdout" "$artifacts/success.expected"
          assert_stderr_contains "$artifacts/success.stderr" "diagnostic"

          NIXFIED_STATE_DIR="$inner/accepted" \
            nixfied-runtime run --model "$TASK_OUTPUT_MODEL/model.json" \
              --task accepted --output task-output \
              >"$artifacts/accepted.stdout" 2>"$artifacts/accepted.stderr"
          assert_stdout 'accepted' \
            "$artifacts/accepted.stdout" "$artifacts/accepted.expected"
          assert_stderr_contains "$artifacts/accepted.stderr" "nonzero"

          NIXFIED_STATE_DIR="$inner/redacted" \
            nixfied-runtime run --model "$TASK_OUTPUT_MODEL/model.json" \
              --task redacted --output task-output \
              >"$artifacts/redacted.stdout" 2>"$artifacts/redacted.stderr"
          assert_stdout '[REDACTED]' \
            "$artifacts/redacted.stdout" "$artifacts/redacted.expected"
          ! [[ "$(<"$artifacts/redacted.stdout")" == *"$NIXFIED_TASK_OUTPUT_SECRET"* ]]
          ! [[ "$(<"$artifacts/redacted.stderr")" == *"$NIXFIED_TASK_OUTPUT_SECRET"* ]]

          timeout_code=0
          if NIXFIED_STATE_DIR="$inner/timeout" \
             nixfied-runtime run --model "$TASK_OUTPUT_MODEL/model.json" \
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
            nixfied-runtime run --model "$TASK_OUTPUT_MODEL/model.json" \
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
             nixfied-runtime run --model "$TASK_OUTPUT_MODEL/model.json" \
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
             nixfied-runtime run --model "$MINIMAL_MODEL/model.json" --output json \
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
             nixfied-runtime run --model "$MINIMAL_MODEL/model.json" --task does-not-exist \
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
             nixfied-runtime run --model "$NEGATIVE_FAIL_MODEL/model.json" --task failing --output json \
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
            nixfied-runtime run --model "$MINIMAL_MODEL/model.json" --task smoke >/dev/null
          root="''${stateDir}/lifecycle-inner-state/minimal/dev/0"
          touch "$root/sentinel"
          jq -r .computedModelHash "$root/.nixfied-state.json" \
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
          root="''${stateDir}/lifecycle-inner-state/minimal/dev/0"
          marker="$root/.nixfied-state.json"
          hash1=$(cat "''${stateDir}/gate-artifacts/lifecycle-hash1.txt")
          NIXFIED_STATE_DIR="''${stateDir}/lifecycle-inner-state" \
            nixfied-runtime run --model "$MINIMAL_MODEL/model.json" --task smoke >/dev/null
          [ -e "$root/sentinel" ] \
            || { echo "lifecycle: second run lost the sentinel file" >&2; exit 1; }
          [ "$(jq -r .computedModelHash "$marker")" = "$hash1" ] \
            || { echo "lifecycle: second run rewrote marker provenance" >&2; exit 1; }
        ''
      ];
    };
  };

  nixfied.tasks.lifecycle-upgrade-preserve = {
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
          root="''${stateDir}/lifecycle-inner-state/minimal/dev/0"
          marker="$root/.nixfied-state.json"
          hash1=$(cat "''${stateDir}/gate-artifacts/lifecycle-hash1.txt")
          NIXFIED_STATE_DIR="''${stateDir}/lifecycle-inner-state" \
            nixfied-runtime run --model "$MINIMAL_B_MODEL/model.json" --task smoke >/dev/null
          [ -e "$root/sentinel" ] \
            || { echo "lifecycle: same-epoch upgrade cleaned the state root" >&2; exit 1; }
          hash2=$(jq -r .computedModelHash "$marker")
          [ "$hash2" != "$hash1" ] \
            || { echo "lifecycle: upgrade did not rewrite provenance" >&2; exit 1; }
          echo "$hash2" > "''${stateDir}/gate-artifacts/lifecycle-hash2.txt"
        ''
      ];
    };
  };

  nixfied.tasks.lifecycle-upgrade-epoch = {
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
          root="''${stateDir}/lifecycle-inner-state/minimal/dev/0"
          marker="$root/.nixfied-state.json"
          NIXFIED_STATE_DIR="''${stateDir}/lifecycle-inner-state" \
            nixfied-runtime run --model "$MINIMAL_EPOCH2_MODEL/model.json" --task smoke >/dev/null
          [ ! -e "$root/sentinel" ] \
            || { echo "lifecycle: epoch upgrade preserved state across the declared boundary" >&2; exit 1; }
          [ "$(jq -r .stateEpoch "$marker")" = "2" ] \
            || { echo "lifecycle: epoch upgrade did not record the new epoch" >&2; exit 1; }
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
          marker="''${stateDir}/lifecycle-inner-state/minimal/dev/0/.nixfied-state.json"
          mkdir -p "''${stateDir}/gate-artifacts"
          jq '.projectId = "intruder"' "$marker" > "$marker.tmp" && mv "$marker.tmp" "$marker"
          code=0
          NIXFIED_STATE_DIR="''${stateDir}/lifecycle-inner-state" \
            nixfied-runtime run --model "$MINIMAL_EPOCH2_MODEL/model.json" --task smoke \
            >/dev/null 2>"''${stateDir}/gate-artifacts/lifecycle-tamper-owner.json" || code=$?
          [ "$code" -eq 21 ] \
            || { echo "lifecycle: tampered ownership exited $code, want 21 (STATE_UNOWNED)" >&2; exit 1; }
          jq '.projectId = "minimal" | .runtimeAbi = "nixfied-runtime-abi:0-foreign"' \
            "$marker" > "$marker.tmp" && mv "$marker.tmp" "$marker"
          code=0
          NIXFIED_STATE_DIR="''${stateDir}/lifecycle-inner-state" \
            nixfied-runtime run --model "$MINIMAL_EPOCH2_MODEL/model.json" --task smoke \
            >/dev/null 2>"''${stateDir}/gate-artifacts/lifecycle-tamper-abi.json" || code=$?
          [ "$code" -eq 21 ] \
            || { echo "lifecycle: tampered runtime ABI exited $code, want 21 (STATE_UNOWNED)" >&2; exit 1; }
        ''
      ];
    };
  };

  nixfied.tasks.lifecycle-service-lifetime = {
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
          inner="''${stateDir}/service-lifetime-inner"
          mkdir -p "''${stateDir}/gate-artifacts" "$inner"
          NIXFIED_STATE_DIR="$inner" \
            nixfied-runtime run --model "$PERSISTENT_ENDPOINT_MODEL/model.json" \
              --task keep-up --timeout-ms 60000 --output json \
            > "''${stateDir}/gate-artifacts/service-lifetime-up.json"
          NIXFIED_STATE_DIR="$inner" \
            nixfied-runtime ps --model "$PERSISTENT_ENDPOINT_MODEL/model.json" \
            > "''${stateDir}/gate-artifacts/service-lifetime-ps-standing.json"
          jq -e '.processes[] | select((has("serviceStatus") | not) and .registryStatus == "ready" and .reconciledStatus == "running" and .serviceLifetime == "persistent-until-down" and .live == true and .borrowerCount == 0)' \
            "''${stateDir}/gate-artifacts/service-lifetime-ps-standing.json" >/dev/null

          NIXFIED_STATE_DIR="$inner" \
            nixfied-runtime run --model "$PERSISTENT_ENDPOINT_MODEL/model.json" \
              --task smoke --timeout-ms 60000 --output json \
            > "''${stateDir}/gate-artifacts/service-lifetime-borrow.json"
          owner_instance=$(jq -r '.services[0].serviceInstanceId' \
            "''${stateDir}/gate-artifacts/service-lifetime-up.json")
          owner_process=$(jq -r '.services[0].processKey' \
            "''${stateDir}/gate-artifacts/service-lifetime-up.json")
          borrower_instance=$(jq -r '.services[0].serviceInstanceId' \
            "''${stateDir}/gate-artifacts/service-lifetime-borrow.json")
          borrower_process=$(jq -r '.services[0].processKey' \
            "''${stateDir}/gate-artifacts/service-lifetime-borrow.json")
          [ "$borrower_instance" = "$owner_instance" ] \
            || { echo "service lifetime: borrower did not reuse the standing service instance" >&2; exit 1; }
          [ "$borrower_process" = "$owner_process" ] \
            || { echo "service lifetime: borrower did not reuse the standing process" >&2; exit 1; }
          NIXFIED_STATE_DIR="$inner" \
            nixfied-runtime ps --model "$PERSISTENT_ENDPOINT_MODEL/model.json" \
            > "''${stateDir}/gate-artifacts/service-lifetime-ps-released.json"
          jq -e --arg id "$owner_instance" \
            '.processes[] | select(.serviceInstanceId == $id and (has("serviceStatus") | not) and .registryStatus == "ready" and .reconciledStatus == "running" and .serviceLifetime == "persistent-until-down" and .live == true and .borrowerCount == 0)' \
            "''${stateDir}/gate-artifacts/service-lifetime-ps-released.json" >/dev/null

          NIXFIED_STATE_DIR="$inner" \
            nixfied-runtime down --model "$PERSISTENT_ENDPOINT_MODEL/model.json" \
            > "''${stateDir}/gate-artifacts/service-lifetime-down.json"
          jq -e '.stopped | length == 1' \
            "''${stateDir}/gate-artifacts/service-lifetime-down.json" >/dev/null
          NIXFIED_STATE_DIR="$inner" \
            nixfied-runtime ps --model "$PERSISTENT_ENDPOINT_MODEL/model.json" \
            > "''${stateDir}/gate-artifacts/service-lifetime-ps-stopped.json"
          jq -e --arg id "$owner_instance" \
            '[.processes[] | select(.serviceInstanceId == $id and .live == true)] | length == 0' \
            "''${stateDir}/gate-artifacts/service-lifetime-ps-stopped.json" >/dev/null
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
            nixfied-runtime run --model "$PURGE_MINIMAL_MODEL/model.json" \
              --task smoke --timeout-ms 60000 \
            > "''${stateDir}/gate-artifacts/purge-run.json"
          if NIXFIED_STATE_DIR="$inner" \
             nixfied-runtime clean --model "$PURGE_MINIMAL_MODEL/model.json" \
             >"''${stateDir}/gate-artifacts/purge-clean-standard.json" \
             2>"''${stateDir}/gate-artifacts/purge-clean-standard.err"; then
            echo "purge: standard clean accepted protected persistent state" >&2
            exit 1
          fi
          NIXFIED_STATE_DIR="$inner" \
            nixfied-runtime clean --model "$PURGE_MINIMAL_MODEL/model.json" --purge \
            > "''${stateDir}/gate-artifacts/purge-clean.json"
          deleted=$(jq -r '.deletedPath' "''${stateDir}/gate-artifacts/purge-clean.json")
          jq -e '.cleanupId and .deletedPath' \
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
          cleanup_endpoint_roots() {
            NIXFIED_STATE_DIR="$root_a" \
              nixfied-runtime down --model "$PERSISTENT_ENDPOINT_MODEL/model.json" \
              >/dev/null 2>&1 || true
            NIXFIED_STATE_DIR="$root_b" \
              nixfied-runtime down --model "$PERSISTENT_ENDPOINT_MODEL/model.json" \
              >/dev/null 2>&1 || true
          }
          trap cleanup_endpoint_roots EXIT

          NIXFIED_STATE_DIR="$root_a" \
            nixfied-runtime run --model "$PERSISTENT_ENDPOINT_MODEL/model.json" \
              --task keep-up --timeout-ms 60000 --output json \
            > "$artifacts/endpoint-root-a.json"
          [ "$(find "$root_a" -name endpoint-prepare-sentinel -type f | wc -l)" -eq 1 ] \
            || { echo "endpoint: root A prepare sentinel missing" >&2; exit 1; }

          if NIXFIED_STATE_DIR="$root_b" \
             nixfied-runtime run --model "$PERSISTENT_ENDPOINT_MODEL/model.json" \
               --task keep-up --timeout-ms 60000 --output json \
             >/dev/null 2>"$artifacts/endpoint-root-b-conflict.json"; then
            echo "endpoint: independent root B took root A's live listener" >&2
            exit 1
          fi
          tail -n 1 "$artifacts/endpoint-root-b-conflict.json" \
            | jq -e '
                .code == "PORT_CONFLICT"
                and .details.portConflict.reason == "listener-occupied"
                and .details.portConflict.endpoint.endpointId == "synthetic-tcp"
              ' >/dev/null
          [ "$(find "$root_b" -name endpoint-prepare-sentinel -type f | wc -l)" -eq 0 ] \
            || { echo "endpoint: root B prepared before conflict refusal" >&2; exit 1; }

          NIXFIED_STATE_DIR="$root_a" \
            nixfied-runtime down --model "$PERSISTENT_ENDPOINT_MODEL/model.json" >/dev/null
          NIXFIED_STATE_DIR="$root_b" \
            nixfied-runtime run --model "$PERSISTENT_ENDPOINT_MODEL/model.json" \
               --task keep-up --timeout-ms 60000 --output json \
            > "$artifacts/endpoint-root-b.json"
          [ "$(find "$root_b" -name endpoint-prepare-sentinel -type f | wc -l)" -eq 1 ] \
            || { echo "endpoint: root B did not prepare after root A went down" >&2; exit 1; }
          NIXFIED_STATE_DIR="$root_b" \
            nixfied-runtime down --model "$PERSISTENT_ENDPOINT_MODEL/model.json" >/dev/null
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
            nixfied-runtime run --model "$DOWNSTREAM_MODEL/model.json" \
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
            nixfied-runtime run --model "$DOWNSTREAM_MODEL/model.json" \
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
            nixfied-runtime clean --model "$DOWNSTREAM_MODEL/model.json" --slot 0 \
            > "''${stateDir}/gate-artifacts/slots-clean-0.json"
          NIXFIED_STATE_DIR="''${stateDir}/slots-inner" \
            nixfied-runtime clean --model "$DOWNSTREAM_MODEL/model.json" --slot 1 \
            > "''${stateDir}/gate-artifacts/slots-clean-1.json"
          clean0=$(jq -r '.deletedPath' "''${stateDir}/gate-artifacts/slots-clean-0.json")
          clean1=$(jq -r '.deletedPath' "''${stateDir}/gate-artifacts/slots-clean-1.json")
          jq -e '.cleanupId and .deletedPath' \
            "''${stateDir}/gate-artifacts/slots-clean-0.json" >/dev/null
          jq -e '.cleanupId and .deletedPath' \
            "''${stateDir}/gate-artifacts/slots-clean-1.json" >/dev/null
          [ -n "$clean0" ] && [ "$clean0" != "null" ] \
            || { echo "slots: clean slot 0 did not report a deletedPath" >&2; exit 1; }
          [ -n "$clean1" ] && [ "$clean1" != "null" ] \
            || { echo "slots: clean slot 1 did not report a deletedPath" >&2; exit 1; }
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
      example-polyglot.task = "example-polyglot";
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
      "lifecycle-upgrade-preserve"
      "lifecycle-upgrade-epoch"
      "lifecycle-tamper-refusal"
      "lifecycle-service-lifetime"
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
