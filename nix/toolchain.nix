# Pinned Rust toolchain variants shared across build and dev contexts.
# Import this once per pkgs instance; Nix deduplicates the store path.
{ pkgs }:
let
  base = pkgs.rust-bin.stable."1.96.0".minimal;
in
{
  # Compiler + cargo only — no linting tools. Used by buildRustPackage.
  build = base;
  # Adds clippy and rustfmt. Used by the hermetic check gate, cargo test
  # shell apps, and the devShell.
  dev = base.override {
    extensions = [
      "clippy"
      "rustfmt"
    ];
  };
}
