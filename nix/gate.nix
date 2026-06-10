# Convenience launcher for the self-hosted conformance gate.
#
# `nix run .#gate` rebuilds the runtime + self-model from the current working
# tree, runs the fast `check` launch-sanity step, then runs the `conformance`
# workflow. This is only a launcher: the orchestration (ordering, readiness gates,
# per-check assertions) is the nixfied runtime running its own workflow, exactly
# as CI does. Extra args are forwarded to `run` (e.g. `nix run .#gate -- --slot 1`).
#
# Run it from the repo root so the working tree is the checkout under test (the
# adoption check installs/upgrades `path:$PWD`). The run uses a fixed state dir
# that is wiped at the start of each run (so repeated runs never collide or hit a
# stale state marker) and KEPT afterward, so the run summary, logs, and
# ground-truth artifacts it reports remain inspectable until the next run.
{
  pkgs,
  runtime,
  model,
}:
pkgs.writeShellApplication {
  name = "nixfied-gate";
  runtimeInputs = [
    pkgs.nix
    pkgs.git
  ];
  text = ''
    runtime="${runtime}/bin/nixfied-runtime"
    model="${model}/model.json"

    # A fixed, predictable state dir: wiped fresh at the start of every run, kept
    # afterward so the reported paths (workflow summary, task logs, ground-truth
    # verdicts) survive for inspection.
    state="''${TMPDIR:-/tmp}/nixfied-gate"
    rm -rf "$state"
    mkdir -p "$state"
    export NIXFIED_STATE_DIR="$state"
    export NIXFIED_CONFORMANCE_ARTIFACTS="$state/conformance-artifacts"
    export NIXFIED_CONFORMANCE_CHECKOUT="''${NIXFIED_CONFORMANCE_CHECKOUT:-$PWD}"

    echo "==> check (launch sanity)" >&2
    "$runtime" check --model "$model" >/dev/null
    echo "==> run --workflow conformance" >&2
    echo "    state + ground-truth artifacts: $state" >&2
    exec "$runtime" run \
      --model "$model" \
      --workflow conformance \
      --timeout-ms 600000 \
      "$@"
  '';
}
