# Private upgrade file boundary; isolated from installer/runtime Cargo products.
{ pkgs }:
let
  toolchain = (import ../toolchain.nix { inherit pkgs; }).dev;
  rustPlatform = pkgs.makeRustPlatform {
    cargo = toolchain;
    rustc = toolchain;
  };
in
rustPlatform.buildRustPackage {
  pname = "nixfied-upgrade-files";
  version = "0.1.0";
  src = pkgs.lib.cleanSourceWith {
    src = ./upgrade-helper;
    filter = path: type: !(type == "directory" && builtins.baseNameOf path == "target");
  };
  cargoLock.lockFile = ./upgrade-helper/Cargo.lock;
  preCheck = ''
    cargo fmt --check
    cargo clippy --all-targets -- -D warnings
  '';
}
