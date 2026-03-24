{
  pkgs,
}:
let
  kernelPackage = import ../kernel { inherit pkgs; };
  runtimeArtifactContracts = import ../../contracts/runtime-artifact-contracts.nix { inherit pkgs; };
  validationBundleFile = pkgs.writeText "nixfied-runtime-artifact-contract-bundle.json" (
    builtins.toJSON runtimeArtifactContracts.bundle
  );
in
''
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
    local lock_file
    local lock_fd
    local detail_tmp
    local export_tmp

    events_file="$(registry_events_file "$root")"
    lock_file="$(registry_events_lock_file "$root")"
    mkdir -p "$root"
    lock_fd="$(registry_lock_acquire "$lock_file" "registry-events-append" "$REGISTRY_DEFAULT_LOCK_TIMEOUT_SECONDS")" || return 1
    REGISTRY_APPEND_LAST_SEQ=""
    REGISTRY_APPEND_LAST_EVENT_JSON=""
    detail_tmp="$(mktemp "$events_file.detail.XXXXXX")" || {
      registry_lock_release "$lock_fd" "$lock_file"
      return 1
    }
    export_tmp="$(mktemp "$events_file.export.XXXXXX")" || {
      rm -f "$detail_tmp"
      registry_lock_release "$lock_fd" "$lock_file"
      return 1
    }
    printf '%s\n' "$detail_json" > "$detail_tmp"

    if ${kernelPackage}/bin/nixfied-kernel registry append \
      ${pkgs.lib.escapeShellArg validationBundleFile} \
      "$root" \
      "$run_id" \
      "$attempt_id" \
      "$workflow_id" \
      "$task_id" \
      "$state" \
      "$detail_tmp" \
      "$detail_reason" \
      "$detail_exit_code" \
      "$export_tmp" >/dev/null; then
      # shellcheck source=/dev/null
      source "$export_tmp"
      rc=0
    else
      rc=1
    fi

    rm -f "$detail_tmp" "$export_tmp"
    registry_lock_release "$lock_fd" "$lock_file"
    return "$rc"
  }
''
