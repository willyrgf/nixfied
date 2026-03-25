{ pkgs }:
pkgs.rustPlatform.buildRustPackage {
  pname = "nixfied-kernel";
  version = "0.1.0";
  src = ./.;
  cargoLock.lockFile = ./Cargo.lock;
  meta = {
    description = "Internal runtime kernel for nixfied validation and state operations.";
    license = with pkgs.lib.licenses; [ mit ];
    platforms = pkgs.lib.platforms.unix;
    mainProgram = "nixfied-kernel";
  };
}
