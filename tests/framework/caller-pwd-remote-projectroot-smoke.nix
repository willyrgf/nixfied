{
  pkgs,
  model,
  services,
  registry,
}:
let
  baseTask = model.tasks."task.ci.quality";

  probeTaskId = "task.test.caller-pwd.remote-projectroot";

  probeTask = baseTask // {
    id = probeTaskId;
    summary = "remote caller pwd workdir probe";
    description = "Verifies proxied framework tasks default projectRoot workdir to the caller checkout.";
    runner = {
      type = "shell";
      command = ''
        set -euo pipefail
        echo "INFO: sandbox_pwd=$(pwd -P)"
        echo "OK: caller pwd probe task complete"
      '';
      package = null;
      workflowId = null;
    };
  };

  probeModel = model // {
    tasks = model.tasks // {
      ${probeTaskId} = probeTask;
    };
  };

  executor = import ../../nixfied/framework/runtime/executor.nix {
    inherit
      pkgs
      registry
      ;
    model = probeModel;
    inherit services;
    projectRoot = ../..;
  };
in
pkgs.runCommand "caller-pwd-remote-projectroot-smoke" { } ''
  set -euo pipefail

  EXECUTOR="${executor}/bin/nixfied-executor"
  export REGISTRY_ROOT="$TMPDIR/registry"
  export NIXFIED_RUNTIME_DIR_SCOPE_OVERRIDE="$TMPDIR/runtime-scope"
  mkdir -p "$REGISTRY_ROOT"
  mkdir -p "$NIXFIED_RUNTIME_DIR_SCOPE_OVERRIDE"

  caller_repo="$TMPDIR/caller-repo"
  mkdir -p "$caller_repo/subdir"

  set +e
  NIXFIED_CALLER_PWD="$caller_repo/subdir" "$EXECUTOR" run-task "${probeTaskId}" > "$TMPDIR/probe.out" 2>&1
  probe_rc="$?"
  set -e
  if [ "$probe_rc" -ne 0 ]; then
    echo "probe task failed rc=$probe_rc"
    cat "$TMPDIR/probe.out"
    exit 1
  fi

  sandbox_pwd="$(${pkgs.gnused}/bin/sed -n 's/^INFO: sandbox_pwd=//p' "$TMPDIR/probe.out" | ${pkgs.coreutils}/bin/tail -n 1)"
  if [ "$sandbox_pwd" != "$caller_repo/subdir" ]; then
    echo "expected sandbox workdir to follow caller pwd for store-backed project roots"
    echo "sandbox_pwd=$sandbox_pwd"
    cat "$TMPDIR/probe.out"
    exit 1
  fi

  ${pkgs.gnugrep}/bin/grep -Fq "OK: caller pwd probe task complete" "$TMPDIR/probe.out"

  echo "OK: remote projectRoot workdir honors caller pwd by default" > "$out"
''
