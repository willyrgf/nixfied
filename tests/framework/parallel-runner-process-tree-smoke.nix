{
  pkgs,
  model,
  services,
  registry,
}:
let
  executor = import ../../nixfied/framework/runtime/executor.nix {
    inherit
      pkgs
      model
      services
      registry
      ;
    projectRoot = ../..;
  };
in
pkgs.runCommand "parallel-runner-process-tree-smoke" { } ''
  set -euo pipefail

  EXECUTOR="${executor}/bin/nixfied-executor"
  export REGISTRY_ROOT="$TMPDIR/registry"
  export CI_ARTIFACTS_ROOT="$TMPDIR/artifacts"
  export NIXFIED_RUNTIME_DIR_SCOPE_OVERRIDE="$TMPDIR/runtime-scope"
  export NIXFIED_WORKFLOW_PARALLEL=1
  export NIXFIED_PARALLEL_CHILD_LEAK_DIR="$TMPDIR/parallel-child-leak"
  mkdir -p "$REGISTRY_ROOT" "$CI_ARTIFACTS_ROOT" "$NIXFIED_RUNTIME_DIR_SCOPE_OVERRIDE"

  set +e
  "$EXECUTOR" run-workflow workflow.test.parallel.failfast --summary > "$TMPDIR/failfast.out" 2>&1
  failfast_rc="$?"
  set -e
  if [ "$failfast_rc" -eq 0 ]; then
    echo "expected failfast workflow to fail"
    cat "$TMPDIR/failfast.out"
    exit 1
  fi

  child_pid_file="$NIXFIED_PARALLEL_CHILD_LEAK_DIR/slow-a.pid"
  for _ in 1 2 3 4 5; do
    if [ -f "$child_pid_file" ]; then
      break
    fi
    sleep 1
  done

  if [ ! -f "$child_pid_file" ]; then
    echo "missing slow-a child pid file"
    cat "$TMPDIR/failfast.out"
    exit 1
  fi

  child_pid="$(${pkgs.coreutils}/bin/tr -d '\n' < "$child_pid_file")"
  if [ -z "$child_pid" ]; then
    echo "missing child pid"
    cat "$TMPDIR/failfast.out"
    exit 1
  fi

  sleep 1
  if kill -0 "$child_pid" 2>/dev/null; then
    echo "parallel fail-fast cancellation left child process running pid=$child_pid"
    ${pkgs.procps}/bin/ps -p "$child_pid" -o pid=,ppid=,command=
    cat "$TMPDIR/failfast.out"
    exit 1
  fi

  echo "OK: parallel fail-fast cancellation tears down child processes, not only wrapper pids" > "$out"
''
