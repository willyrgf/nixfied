{ pkgs }:
pkgs.stdenv.mkDerivation {
  pname = "nixfied-kernel";
  version = "0.1.0";
  src = ./.;

  nativeBuildInputs = [ pkgs.rustc ];

  dontConfigure = true;

  buildPhase = ''
    runHook preBuild
    rustc --edition=2021 src/main.rs -O -o nixfied-kernel
    runHook postBuild
  '';

  installPhase = ''
    runHook preInstall
    mkdir -p "$out/bin"
    cp nixfied-kernel "$out/bin/nixfied-kernel"
    runHook postInstall
  '';

  meta = {
    description = "Internal runtime kernel for nixfied validation and state operations.";
    license = with pkgs.lib.licenses; [ mit ];
    platforms = pkgs.lib.platforms.unix;
    mainProgram = "nixfied-kernel";
  };
}

