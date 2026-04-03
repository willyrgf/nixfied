{ pkgs, model, projectRoot, modelFile }:
let
  kernelPackage = import ./kernel { inherit pkgs; };
in
''
  MODEL_FILE=${pkgs.lib.escapeShellArg (builtins.toString modelFile)}
  export NIXFIED_MODEL_FILE="$MODEL_FILE"
  PROJECT_ROOT=${pkgs.lib.escapeShellArg (builtins.toString projectRoot)}
  REGISTRY_ROOT_DEFAULT="${model.state.policy.registryRoot}"
  ARTIFACTS_ROOT_DEFAULT="${model.state.policy.artifactsRoot}"
  if [ -n "''${NIXFIED_RUNTIME_REGISTRY_ROOT+x}" ]; then
    REGISTRY_ROOT_DEFAULT="$NIXFIED_RUNTIME_REGISTRY_ROOT"
  elif [ -n "''${NIXFIED_RUNTIME_DIR_BASE+x}" ]; then
    REGISTRY_ROOT_DEFAULT="$NIXFIED_RUNTIME_DIR_BASE/registry"
  fi
  if [ -n "''${NIXFIED_RUNTIME_ARTIFACTS_DIR+x}" ]; then
    ARTIFACTS_ROOT_DEFAULT="$NIXFIED_RUNTIME_ARTIFACTS_DIR"
  elif [ -n "''${NIXFIED_RUNTIME_DIR_BASE+x}" ]; then
    ARTIFACTS_ROOT_DEFAULT="$NIXFIED_RUNTIME_DIR_BASE/artifacts"
  fi
  if [ -n "''${REGISTRY_ROOT+x}" ]; then
    REGISTRY_ROOT_EXPLICIT=1
  else
    REGISTRY_ROOT_EXPLICIT=0
  fi
  REGISTRY_ROOT="''${REGISTRY_ROOT:-$REGISTRY_ROOT_DEFAULT}"
  RUN_ID_ACTIVE_ROOT="$REGISTRY_ROOT/active"
  RUN_ID_COUNTER_ROOT="$REGISTRY_ROOT/counters"

  sha256_text() {
    printf '%s' "$1" | ${pkgs.coreutils}/bin/sha256sum | ${pkgs.gawk}/bin/awk '{print $1}'
  }

  compute_attempt_id() {
    local attempt_dir
    local attempt_name

    attempt_dir="$(mktemp -d "''${TMPDIR:-/tmp}/nixfied-attempt.XXXXXX")" || return 1
    attempt_name="$(basename "$attempt_dir")"
    rmdir "$attempt_dir"
    printf '%s' "attempt-''${attempt_name#nixfied-attempt.}"
  }

  kernel_event_detail() {
    ${kernelPackage}/bin/nixfied-kernel event-detail render "$@"
  }
''
