{
  lib,
  rustPlatform,
  fetchFromGitHub,
}:
rustPlatform.buildRustPackage rec {
  pname = "helios";
  version = "0.8.4-nightly-fa2d703";

  src = fetchFromGitHub {
    owner = "a16z";
    repo = "helios";
    rev = "fa2d7034125bdb79cb16bcce3bac7c476160edce";
    hash = "sha256-Z6NA7sYFJwBB63PMd6N+eZqqW67/z0G7lbmEY1JR6z0=";
  };

  cargoHash = "sha256-9869eTna8tL6lIRIDvAoqUxSoXhbZSzubmtnbR+bM/k=";
  cargoBuildFlags = [
    "-p"
    "helios-cli"
    "--bin"
    "helios"
  ];

  doCheck = false;

  meta = with lib; {
    description = "A fast, secure, and portable light client for Ethereum";
    homepage = "https://github.com/a16z/helios";
    license = licenses.mit;
    mainProgram = "helios";
    platforms = platforms.unix;
  };
}
