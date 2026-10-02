# Compile independent socket peers and a native-command fixture; run outside
# the Nix sandbox through .#test because the suite binds loopback listeners.
{ pkgs }:
let
  toolchain = (import ../toolchain.nix { inherit pkgs; }).dev;
  nativeFixture = pkgs.runCommand "reth-peer-native-fixture" { nativeBuildInputs = [ toolchain ]; } ''
    mkdir -p "$out/bin"
    rustc --edition=2024 -D warnings ${./reth-peer-probe.rs} -o "$out/bin/reth"
  '';
  fixtureProbe = import ../adapters/reth-probe.nix {
    pkgs = pkgs // { reth = nativeFixture; };
  };
  realProbe = import ../adapters/reth-probe.nix { inherit pkgs; };
in
pkgs.runCommand "nixfied-reth-peer-tests" {
  nativeBuildInputs = [ toolchain ];
  NIXFIED_TEST_RETH_PROBE = "${fixtureProbe}/bin/nixfied-reth-probe";
  NIXFIED_TEST_REAL_RETH_PROBE = "${realProbe}/bin/nixfied-reth-probe";
  NIXFIED_TEST_CURL = "${pkgs.curl}/bin/curl";
  NIXFIED_TEST_RETH = "${pkgs.reth}/bin/reth";
} ''
  rustfmt --edition=2024 --check ${./reth-peer-probe.rs}
  mkdir -p "$out/bin"
  rustc --edition=2024 --test -D warnings ${./reth-peer-probe.rs} -o "$out/bin/nixfied-reth-peer-tests"
''
