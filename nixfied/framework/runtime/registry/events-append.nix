{
  pkgs,
  registryEventPayloadExpr,
}:
let
  lib = pkgs.lib;
  runtimeArtifactContracts = import ../../contracts/runtime-artifact-contracts.nix { inherit pkgs; };
  registryEventValidator = import ../../contracts/mkValidator.nix {
    inherit
      pkgs
      ;
    contractBundle = runtimeArtifactContracts;
    contractRef = "runtime.registryEvent";
  };
in
''
  registry_next_seq() {
    local root="$1"
    local seq_file
    local lock_file
    local next="1"
    local lock_fd
    local tmp_file

    seq_file="$(registry_seq_file "$root")"
    lock_file="$(registry_seq_lock_file "$root")"
    mkdir -p "$root"
    lock_fd="$(registry_lock_acquire "$lock_file" "registry-seq" "$REGISTRY_DEFAULT_LOCK_TIMEOUT_SECONDS")" || return 1
    if [ -f "$seq_file" ]; then
      next="$(( $(cat "$seq_file") + 1 ))"
    fi
    tmp_file="$(mktemp "$seq_file.tmp.XXXXXX")"
    printf '%s' "$next" > "$tmp_file"
    mv "$tmp_file" "$seq_file"
    registry_lock_release "$lock_fd" "$lock_file"

    printf '%s' "$next"
  }

  registry_append_event() {
    local root="$1"
    local run_id="$2"
    local attempt_id="$3"
    local workflow_id="$4"
    local task_id="$5"
    local state="$6"
    local detail_json="$7"
    local detail_reason="''${8:-}"
    local detail_exit_code="''${9:-}"

    local events_file
    local events_index_file
    local seq_file
    local lock_file
    local lock_fd
    local seq
    local ts
    local ts_epoch
    local rc
    local seq_tmp
    local event_tmp
    local validate_stderr

    events_file="$(registry_events_file "$root")"
    events_index_file="$(registry_events_index_file "$root")"
    seq_file="$(registry_seq_file "$root")"
    lock_file="$(registry_events_lock_file "$root")"
    mkdir -p "$root"
    lock_fd="$(registry_lock_acquire "$lock_file" "registry-events-append" "$REGISTRY_DEFAULT_LOCK_TIMEOUT_SECONDS")" || return 1
    if [ -f "$seq_file" ]; then
      seq="$(( $(cat "$seq_file") + 1 ))"
    else
      seq="1"
    fi
    seq_tmp="$(mktemp "$seq_file.tmp.XXXXXX")"
    printf '%s' "$seq" > "$seq_tmp"
    mv "$seq_tmp" "$seq_file"
    ts="$(date -u +"$REGISTRY_TIMESTAMP_FORMAT")"
    ts_epoch="$(date +%s)"
    event_tmp="$(mktemp "$events_file.event.XXXXXX")"
    validate_stderr="$(mktemp "$events_file.validate.XXXXXX")"
    REGISTRY_APPEND_LAST_SEQ=""
    REGISTRY_APPEND_LAST_EVENT_JSON=""

    set +e
    ${pkgs.jq}/bin/jq -cnS \
      --arg kind "$REGISTRY_EVENT_KIND" \
      --argjson version "$REGISTRY_EVENT_VERSION" \
      --argjson seq "$seq" \
      --arg ts "$ts" \
      --arg runId "$run_id" \
      --arg attemptId "$attempt_id" \
      --arg workflowId "$workflow_id" \
      --arg taskId "$task_id" \
      --arg state "$state" \
      --argjson detail "$detail_json" \
      ${lib.escapeShellArg registryEventPayloadExpr} \
      > "$event_tmp"
    rc="$?"
    set -e

    if [ "$rc" -eq 0 ]; then
      if ${registryEventValidator} "$event_tmp" >/dev/null 2>"$validate_stderr"; then
        REGISTRY_APPEND_LAST_SEQ="$seq"
        REGISTRY_APPEND_LAST_EVENT_JSON="$(cat "$event_tmp")"
        cat "$event_tmp" >> "$events_file"
        rc="$?"
        if [ "$rc" -eq 0 ]; then
          printf '%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\n' \
            "$seq" \
            "$ts_epoch" \
            "$ts" \
            "$run_id" \
            "$attempt_id" \
            "$workflow_id" \
            "$task_id" \
            "$state" \
            "$detail_reason" \
            "$detail_exit_code" >> "$events_index_file"
          rc="$?"
        fi
      else
        cat "$validate_stderr" >&2 || true
        rc=1
      fi
    fi

    rm -f "$event_tmp" "$validate_stderr"
    registry_lock_release "$lock_fd" "$lock_file"
    return "$rc"
  }
''
