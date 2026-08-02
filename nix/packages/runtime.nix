# Reproducible, host-toolchain-free build of one Nixfied workspace product.
# Source filtering keeps unrelated workspace members out of the product's
# derivation identity; the shared lockfile remains the canonical dependency input.
#
# `buildType` selects the cargo profile: the default `"release"` is what
# generated adopter apps use; the framework's own CI path (flake checks and the
# gate) builds `"debug"` instead.
#
# `pkgs` must carry the rust-overlay overlay (it provides `rust-bin`). The
# toolchain is pinned >= the workspace rust-version; rusqlite's `bundled` feature
# compiles SQLite from the nix stdenv C toolchain (no system sqlite/pkg-config).
{
  pkgs,
  package,
  buildType ? "release",
  source ? ../../runtime,
}:
let
  rustToolchain = (import ../toolchain.nix { inherit pkgs; }).build;
  rustPlatform = pkgs.makeRustPlatform {
    cargo = rustToolchain;
    rustc = rustToolchain;
  };
  canonicalLock = builtins.toFile "nixfied-Cargo.lock" (builtins.readFile ../../runtime/Cargo.lock);
  sharedCargoDeps = rustPlatform.importCargoLock {
    lockFile = canonicalLock;
  };
  sourceInfo = import ./runtime-source.nix {
    inherit pkgs source package;
  };
in
rustPlatform.buildRustPackage {
  pname = package;
  version = "0.1.0";
  src = sourceInfo.root;
  cargoLock.lockFile = canonicalLock;
  cargoDeps = sharedCargoDeps;
  # The canonical lock/vendor pair is validated first. Then Cargo derives the
  # selected workspace lock after that hook, so `cargo metadata --locked` and the
  # package build see only the members in this filtered root.
  preConfigure = ''
    cargo generate-lockfile --offline
  '';
  inherit buildType;
  cargoBuildFlags = [ "--package=${package}" ];
  # The white-box `cargo test` floor runs outside the build sandbox (it binds
  # ports and spawns process groups); here we only compile.
  doCheck = false;
}
