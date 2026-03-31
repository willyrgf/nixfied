{
  lib,
  rustPlatform,
  fetchFromGitHub,
  pkg-config,
  perl,
}:
rustPlatform.buildRustPackage {
  pname = "helios";
  version = "0.11.1-nightly-204c998";

  src = fetchFromGitHub {
    owner = "a16z";
    repo = "helios";
    rev = "204c998a927348e1c000a664f08d5b37b1b0d924";
    hash = "sha256-PCDQKoF9EbhPdW0/br725RJgcdkPzt9dGXZIYpFSH7g=";
  };

  cargoHash = "sha256-6ssu32jTArgyXCVWAulL2hT6SoaTxgfvsR22/ozDM0Y=";

  patches = [
    ../../modules/services/runtime/helios/patches/0001-disable-reqwest-hickory-dns.patch
    ../../modules/services/runtime/helios/patches/0002-limit-light-client-updates-request.patch
  ];

  cargoBuildFlags = [
    "--package"
    "helios-cli"
    "--bin"
    "helios"
  ];

  doCheck = false;

  nativeBuildInputs = [
    pkg-config
    perl
  ];

  meta = with lib; {
    description = "A fast, secure, and portable light client for Ethereum";
    homepage = "https://github.com/a16z/helios";
    license = licenses.mit;
    mainProgram = "helios";
    platforms = platforms.unix;
  };
}
