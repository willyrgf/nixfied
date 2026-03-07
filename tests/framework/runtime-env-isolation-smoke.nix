{
  pkgs,
  model,
  registry,
}:
let
  baseTask = model.tasks."task.ci.quality";

  probeTaskId = "task.test.runtime-env.probe";

  probeTask = baseTask // {
    id = probeTaskId;
    summary = "runtime env probe";
    description = "Prints runtime-owned mutable directory env vars for validation.";
    runner = {
      type = "shell";
      command = ''
        set -euo pipefail
        echo "INFO: sandbox_home=$HOME"
        echo "INFO: sandbox_tmp=$TMPDIR"
        echo "INFO: sandbox_xdg_data=$XDG_DATA_HOME"
        echo "INFO: sandbox_xdg_state=$XDG_STATE_HOME"
        echo "INFO: sandbox_xdg_cache=$XDG_CACHE_HOME"
        echo "INFO: sandbox_registry=$REGISTRY_ROOT"
        echo "INFO: sandbox_artifacts=$CI_ARTIFACTS_DIR"
        echo "INFO: sandbox_services=$NIXFIED_SERVICE_ROOT"
        echo "OK: runtime env probe complete"
      '';
      package = null;
      workflowId = null;
    };
    ui = baseTask.ui // {
      app = baseTask.ui.app // {
        expose = false;
        name = "test-runtime-env-probe";
      };
    };
  };

  probeModel = model // {
    tasks = model.tasks // {
      ${probeTaskId} = probeTask;
    };
  };

  harness = import ./lib/harness.nix {
    inherit
      pkgs
      registry
      ;
    model = probeModel;
    projectRoot = ../..;
  };
in
pkgs.runCommand "runtime-env-isolation-smoke" { } ''
  set -euo pipefail
  ${harness.shellPrelude}

  EXECUTOR="${harness.executor}/bin/nixfied-executor"
  runtime_base=${pkgs.lib.escapeShellArg probeModel.runtime.directories.base}

  host_home="$TMPDIR/host-home"
  host_tmp="$TMPDIR/host-tmp"
  host_xdg="$TMPDIR/host-xdg"
  hostile_runtime_root="$TMPDIR/hostile-runtime"
  mkdir -p "$host_home" "$host_tmp" "$host_xdg/data" "$host_xdg/state" "$host_xdg/cache"
  mkdir -p "$TMPDIR/registry" "$TMPDIR/artifacts"

  PROJECT_ENV=prod \
  NIX_ENV=3 \
  HOME="$host_home" \
  TMPDIR="$host_tmp" \
  XDG_DATA_HOME="$host_xdg/data" \
  XDG_STATE_HOME="$host_xdg/state" \
  XDG_CACHE_HOME="$host_xdg/cache" \
  REGISTRY_ROOT="$TMPDIR/registry" \
  CI_ARTIFACTS_DIR="$TMPDIR/artifacts" \
  NIXFIED_RUNTIME_HOME="$hostile_runtime_root/home" \
  NIXFIED_RUNTIME_TMPDIR="$hostile_runtime_root/tmp" \
  NIXFIED_RUNTIME_XDG_DATA_HOME="$hostile_runtime_root/xdg/data" \
  NIXFIED_RUNTIME_XDG_STATE_HOME="$hostile_runtime_root/xdg/state" \
  NIXFIED_RUNTIME_XDG_CACHE_HOME="$hostile_runtime_root/xdg/cache" \
  NIXFIED_RUNTIME_REGISTRY_ROOT="$hostile_runtime_root/registry" \
  NIXFIED_RUNTIME_ARTIFACTS_DIR="$hostile_runtime_root/artifacts" \
  NIXFIED_RUNTIME_SERVICE_ROOT="$hostile_runtime_root/services" \
    "$EXECUTOR" run-task "${probeTaskId}" > "$TMPDIR/probe.out" 2>&1

  ${pkgs.gnugrep}/bin/grep -Fq "INFO: sandbox_home=$runtime_base/prod/slot-3/home" "$TMPDIR/probe.out"
  ${pkgs.gnugrep}/bin/grep -Fq "INFO: sandbox_tmp=$runtime_base/prod/slot-3/tmp" "$TMPDIR/probe.out"
  ${pkgs.gnugrep}/bin/grep -Fq "INFO: sandbox_xdg_data=$runtime_base/prod/slot-3/xdg/data" "$TMPDIR/probe.out"
  ${pkgs.gnugrep}/bin/grep -Fq "INFO: sandbox_xdg_state=$runtime_base/prod/slot-3/xdg/state" "$TMPDIR/probe.out"
  ${pkgs.gnugrep}/bin/grep -Fq "INFO: sandbox_xdg_cache=$runtime_base/prod/slot-3/xdg/cache" "$TMPDIR/probe.out"
  ${pkgs.gnugrep}/bin/grep -Fq "INFO: sandbox_registry=$runtime_base/prod/slot-3/registry" "$TMPDIR/probe.out"
  ${pkgs.gnugrep}/bin/grep -Fq "INFO: sandbox_artifacts=$runtime_base/prod/slot-3/artifacts" "$TMPDIR/probe.out"
  ${pkgs.gnugrep}/bin/grep -Fq "INFO: sandbox_services=$runtime_base/prod/slot-3/services" "$TMPDIR/probe.out"
  ${pkgs.gnugrep}/bin/grep -Fq "OK: runtime env probe complete" "$TMPDIR/probe.out"

  require_not_contains "$TMPDIR/probe.out" "$host_home"
  require_not_contains "$TMPDIR/probe.out" "$host_tmp"
  require_not_contains "$TMPDIR/probe.out" "$host_xdg"
  require_not_contains "$TMPDIR/probe.out" "$hostile_runtime_root"

  echo "OK: runtime-owned env dirs do not inherit host paths" > "$out"
''
