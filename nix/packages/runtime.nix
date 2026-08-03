# Reproducible, host-toolchain-free build of one Nixfied workspace product.
# Source filtering keeps unrelated workspace members out of the product's
# derivation identity. The runtime keeps the canonical workspace lock; the
# dependency-light CLI and test child use package-specific lock/vendor inputs.
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
  lockSources = {
    nixfied-cli = ../../runtime/locks/nixfied-cli.Cargo.lock;
    nixfied-runtime = ../../runtime/Cargo.lock;
    nixfied-test-child = ../../runtime/locks/nixfied-test-child.Cargo.lock;
  };
  lockSource =
    if builtins.hasAttr package lockSources then
      lockSources.${package}
    else
      throw "nixfied package: unsupported package ${package}";
  packageLock = builtins.toFile "nixfied-${package}-Cargo.lock" (builtins.readFile lockSource);
  packageCargoDeps = rustPlatform.importCargoLock {
    lockFile = packageLock;
  };
  sourceInfo = import ./runtime-source.nix {
    inherit pkgs source package;
    lockFile = packageLock;
  };
in
rustPlatform.buildRustPackage {
  pname = package;
  version = "0.1.0";
  src = sourceInfo.root;
  cargoLock.lockFile = packageLock;
  cargoDeps = packageCargoDeps;
  inherit buildType;
  cargoBuildFlags = [ "--package=${package}" ];
  # The white-box `cargo test` floor runs outside the build sandbox (it binds
  # ports and spawns process groups); here we only compile.
  doCheck = false;
}
