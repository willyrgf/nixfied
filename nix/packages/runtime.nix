# Reproducible, host-toolchain-free build of the runtime workspace binaries
# (`nixfied-runtime`, `nixfied`).
#
# `buildType` selects the cargo profile: the default `"release"` is what
# `.#install` ships to adopters; the framework's own CI path (flake checks and the
# gate) builds `"debug"` instead, so every `.#ci` compile shares one fast dev
# profile and no release optimization runs.
#
# `pkgs` must carry the rust-overlay overlay (it provides `rust-bin`). The
# toolchain is pinned >= the workspace rust-version; rusqlite's `bundled` feature
# compiles SQLite from the nix stdenv C toolchain (no system sqlite/pkg-config).
{
  pkgs,
  buildType ? "release",
}:
let
  rustToolchain = (import ../toolchain.nix { inherit pkgs; }).build;
  rustPlatform = pkgs.makeRustPlatform {
    cargo = rustToolchain;
    rustc = rustToolchain;
  };
in
rustPlatform.buildRustPackage {
  pname = "nixfied-runtime";
  version = "0.1.0";
  src = ../../runtime;
  cargoLock.lockFile = ../../runtime/Cargo.lock;
  inherit buildType;
  # The white-box `cargo test` floor runs outside the build sandbox (it binds
  # ports and spawns process groups); here we only compile.
  doCheck = false;
}
