# Developer convenience apps for the framework repo, run against the working tree.
#
# These verify the framework's *own* Rust/Nix source. They are deliberately plain
# shell apps over `nix` + the pinned cargo, not nixfied workflows: a nixfied task
# cannot invoke nix (SEAM-1), and the runtime's own unit tests must not be measured
# by the runtime itself. The adopter surface — where verification *is* a workflow —
# is generated separately by `lib.projectApps`.
{
  pkgs,
  gate,
  runtime,
}:
let
  rustToolchain = pkgs.rust-bin.stable."1.96.0".minimal.override {
    extensions = [
      "clippy"
      "rustfmt"
    ];
  };

  # `.#test`: the white-box cargo floor (binds ports / spawns process groups), so
  # it runs outside the nix sandbox with the pinned toolchain and a C compiler for
  # rusqlite's bundled SQLite.
  test = pkgs.writeShellApplication {
    name = "nixfied-test";
    runtimeInputs = [
      rustToolchain
      pkgs.stdenv.cc
      pkgs.sqlite
      pkgs.git
    ];
    text = ''
      cargo test --manifest-path runtime/Cargo.toml --workspace "$@"
    '';
  };

  # `.#check`: the hermetic source gate (rustfmt/clippy/check + every build) plus
  # the self-model admission sanity the flake checks don't cover.
  check = pkgs.writeShellApplication {
    name = "nixfied-check";
    runtimeInputs = [
      pkgs.nix
      pkgs.git
    ];
    text = ''
      echo "==> nix flake check" >&2
      nix flake check
      echo "==> self-model admission" >&2
      model="$(nix build .#self-model --no-link --print-out-paths)/model.json"
      "${runtime}/bin/nixfied-runtime" check --model "$model"
    '';
  };

  # `.#ci`: the whole repo, fail-fast — source gate, then the impure test floor,
  # then the conformance gate. `writeShellApplication` runs `set -euo pipefail`, so
  # any stage stops the chain.
  ci = pkgs.writeShellApplication {
    name = "nixfied-ci";
    runtimeInputs = [
      pkgs.nix
      pkgs.git
    ];
    text = ''
      echo "==> check" >&2
      ${check}/bin/nixfied-check
      echo "==> test" >&2
      ${test}/bin/nixfied-test
      echo "==> gate" >&2
      ${gate}/bin/nixfied-gate
    '';
  };
in
{
  inherit check test ci;
}
