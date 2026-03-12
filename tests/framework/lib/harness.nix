{
  pkgs,
  model,
  registry,
  projectRoot ? ../../..,
}:
let
  shellHelpers = import ./shell-helpers.nix { inherit pkgs; };
  executor = import ../../../nixfied/framework/runtime/executor.nix {
    inherit
      pkgs
      model
      registry
      projectRoot
      ;
  };

  orchestrator = import ../../../nixfied/framework/runtime/orchestrator.nix {
    inherit
      pkgs
      model
      registry
      projectRoot
      ;
  };
in
{
  inherit executor orchestrator;

  shellPrelude = ''
    ${shellHelpers.shellPrelude}

    wait_for_condition() {
      local timeout_seconds="$1"
      local label="$2"
      shift 2

      local deadline
      deadline=$(( $(${pkgs.coreutils}/bin/date +%s) + timeout_seconds ))

      while true; do
        if "$@"; then
          return 0
        fi
        if [ "$(${pkgs.coreutils}/bin/date +%s)" -ge "$deadline" ]; then
          fail "timed out waiting for $label"
        fi
        sleep 0.1
      done
    }

    wait_for_run_state() {
      local orch="$1"
      local run_id="$2"
      local expected_state="$3"
      local timeout_seconds="$4"

      wait_for_condition "$timeout_seconds" "run state $expected_state for $run_id" \
        check_run_state "$orch" "$run_id" "$expected_state"
    }

    wait_for_run_not_state() {
      local orch="$1"
      local run_id="$2"
      local blocked_state="$3"
      local timeout_seconds="$4"

      wait_for_condition "$timeout_seconds" "run state not $blocked_state for $run_id" \
        check_run_not_state "$orch" "$run_id" "$blocked_state"
    }

    check_run_state() {
      local orch="$1"
      local run_id="$2"
      local expected_state="$3"
      local state

      state="$("$orch" runs "$run_id" | ${pkgs.jq}/bin/jq -r '.state' 2>/dev/null || true)"
      [ "$state" = "$expected_state" ]
    }

    check_run_not_state() {
      local orch="$1"
      local run_id="$2"
      local blocked_state="$3"
      local state

      state="$("$orch" runs "$run_id" | ${pkgs.jq}/bin/jq -r '.state' 2>/dev/null || true)"
      [ -n "$state" ] && [ "$state" != "$blocked_state" ]
    }
  '';
}
