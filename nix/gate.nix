# Convenience launcher for the self-hosted conformance gate.
#
# `nix run .#gate` rebuilds the runtime + self-model from the current working
# tree, runs the fast `check` launch-sanity step, then runs the `conformance`
# workflow. This is only a launcher: the orchestration (ordering, readiness gates,
# per-check assertions) is the nixfied runtime running its own workflow, exactly
# as CI does. Extra args are forwarded to `run` (e.g. `nix run .#gate -- --slot 1`).
#
# Run it from the repo root so the working tree is the checkout under test (the
# adoption check installs/upgrades `path:$PWD`). Each run uses a private throwaway
# state dir, so repeated runs never collide with each other or with prior state.
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
    state="$(mktemp -d "''${TMPDIR:-/tmp}/nixfied-gate.XXXXXX")"
    trap 'rm -rf "$state"' EXIT
    export NIXFIED_STATE_DIR="$state"
    export NIXFIED_CONFORMANCE_CHECKOUT="''${NIXFIED_CONFORMANCE_CHECKOUT:-$PWD}"

    echo "==> check (launch sanity)" >&2
    "$runtime" check --model "$model" >/dev/null
    echo "==> run --workflow conformance" >&2
    "$runtime" run \
      --model "$model" \
      --workflow conformance \
      --timeout-ms 600000 \
      "$@"
  '';
}
