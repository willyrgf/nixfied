# The framework gate coordinator: runtime-layer tests (gate-runtime nixfied model)
# followed by nix-layer tests (gate-nix). No bash functions; all test logic lives
# in the two sub-programs.
#
# `nix run .#gate` rebuilds the debug runtime, all example models, and the
# gate-runtime model from the working tree, then runs both gates sequentially.
{
  pkgs,
  gateNix,
  gateRuntime,
}:
pkgs.writeShellApplication {
  name = "nixfied-gate";
  runtimeInputs = [ pkgs.coreutils ];
  text = ''
    checkout="''${NIXFIED_GATE_CHECKOUT:-$PWD}"
    state="''${TMPDIR:-/tmp}/nixfied-gate"
    rm -rf "$state"
    mkdir -p "$state"
    echo "    state: $state" >&2

    echo "==> gate-runtime" >&2
    NIXFIED_STATE_DIR="$state" ${gateRuntime}/bin/nixfied-gate-runtime

    echo "==> gate-nix" >&2
    NIXFIED_GATE_CHECKOUT="$checkout" \
      ${gateNix}/bin/nixfied-gate-nix "$@"
  '';
}
