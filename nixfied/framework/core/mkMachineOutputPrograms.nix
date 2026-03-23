{
  pkgs,
  appId,
  app,
  contractBundle,
  targetProgram,
  setupPrograms ? [ ],
  teardownPrograms ? [ ],
}:
let
  lib = pkgs.lib;
  mkShellApp = import ./mk-shell-app.nix { inherit pkgs; };
  contractRef = ((app.validation or { }).contractRef or "");
  validatorProgram = import ../contracts/mkValidator.nix {
    inherit
      pkgs
      contractBundle
      contractRef
      ;
  };
  targetArgsLiteral = builtins.concatStringsSep " " (map lib.escapeShellArg (app.targetArgs or [ ]));
  setupProgramsLiteral = builtins.concatStringsSep " " (map lib.escapeShellArg setupPrograms);
  teardownProgramsLiteral = builtins.concatStringsSep " " (map lib.escapeShellArg teardownPrograms);
in
(mkShellApp {
  appName = "machine-output:${appId}";
  binPrefix = "nixfied-machine-output";
  body = ''
    target_program=${lib.escapeShellArg targetProgram}
    target_app_id=${lib.escapeShellArg (app.targetAppId or "")}
    contract_ref=${lib.escapeShellArg contractRef}
    validator_program=${lib.escapeShellArg validatorProgram}
    setup_programs=( ${setupProgramsLiteral} )
    teardown_programs=( ${teardownProgramsLiteral} )
    target_args=( ${targetArgsLiteral} )

    work_dir="$(mktemp -d "''${TMPDIR:-/tmp}/nixfied-machine-output.XXXXXX")"
    cleanup_machine_output() {
      rm -rf "$work_dir"
    }
    trap cleanup_machine_output EXIT

    render_captured_logs() {
      local label="$1"
      local stdout_file="$2"
      local stderr_file="$3"

      if [ -s "$stdout_file" ]; then
        echo "ERROR: $label stdout:" >&2
        cat "$stdout_file" >&2
      fi
      if [ -s "$stderr_file" ]; then
        echo "ERROR: $label stderr:" >&2
        cat "$stderr_file" >&2
      fi
    }

    emit_failure_json() {
      local stage="$1"
      local code="$2"
      local message="$3"
      local failed_app_id="$4"
      local exit_code="$5"

      ${pkgs.jq}/bin/jq -cn \
        --arg appId ${lib.escapeShellArg appId} \
        --arg targetAppId "$target_app_id" \
        --arg stage "$stage" \
        --arg code "$code" \
        --arg message "$message" \
        --arg failedAppId "$failed_app_id" \
        --arg contractRef "$contract_ref" \
        --arg validator "cue" \
        --argjson exitCode "$exit_code" \
        '{
          ok: false,
          appId: $appId,
          targetAppId: $targetAppId,
          stage: $stage,
          code: $code,
          message: $message,
          failedAppId: (if $failedAppId == "" then null else $failedAppId end),
          contractRef: (if $contractRef == "" then null else $contractRef end),
          validator: (if $stage == "validation" then $validator else null end),
          exitCode: $exitCode
        }'
    }

    run_captured_app() {
      local program="$1"
      local stdout_file="$2"
      local stderr_file="$3"
      shift 3
      "$program" "$@" >"$stdout_file" 2>"$stderr_file"
    }

    setup_index=0
    for setup_program in "''${setup_programs[@]}"; do
      setup_index="$((setup_index + 1))"
      setup_stdout="$work_dir/setup-$setup_index.stdout"
      setup_stderr="$work_dir/setup-$setup_index.stderr"
      if run_captured_app "$setup_program" "$setup_stdout" "$setup_stderr"; then
        rc=0
      else
        rc="$?"
        render_captured_logs "setup app $setup_index" "$setup_stdout" "$setup_stderr"
        emit_failure_json "setup" "machine-output-setup-failed" "setup app $setup_index failed" "" "$rc"
        exit "$rc"
      fi
    done

    target_stdout="$work_dir/target.stdout"
    target_stderr="$work_dir/target.stderr"
    if run_captured_app "$target_program" "$target_stdout" "$target_stderr" "''${target_args[@]}" "$@"; then
      rc=0
    else
      rc="$?"
      render_captured_logs "target app" "$target_stdout" "$target_stderr"
      emit_failure_json "target" "machine-output-target-failed" "target app '$target_app_id' failed" "$target_app_id" "$rc"
      exit "$rc"
    fi

    payload_stdout="$work_dir/payload.stdout"
    ${pkgs.gnugrep}/bin/grep -Ev '^(INFO|WARN|ERROR|OK|SKIP): ' "$target_stdout" >"$payload_stdout" || true

    if "$validator_program" "$payload_stdout" >"$work_dir/validate.stdout" 2>"$work_dir/validate.stderr"; then
      :
    else
      render_captured_logs "target app" "$target_stdout" "$target_stderr"
      render_captured_logs "validation" "$work_dir/validate.stdout" "$work_dir/validate.stderr"
      emit_failure_json "validation" "machine-output-validation-failed" "target app '$target_app_id' did not satisfy contract '$contract_ref'" "$target_app_id" 1
      exit 1
    fi

    teardown_index=0
    for teardown_program in "''${teardown_programs[@]}"; do
      teardown_index="$((teardown_index + 1))"
      teardown_stdout="$work_dir/teardown-$teardown_index.stdout"
      teardown_stderr="$work_dir/teardown-$teardown_index.stderr"
      if run_captured_app "$teardown_program" "$teardown_stdout" "$teardown_stderr"; then
        rc=0
      else
        rc="$?"
        render_captured_logs "teardown app $teardown_index" "$teardown_stdout" "$teardown_stderr"
        emit_failure_json "teardown" "machine-output-teardown-failed" "teardown app $teardown_index failed" "" "$rc"
        exit "$rc"
      fi
    done

    cat "$payload_stdout"
  '';
}).program
