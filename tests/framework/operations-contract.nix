{ pkgs }:
let
  source = builtins.readFile ../../nixfied/modules/operations.nix;
in
assert pkgs.lib.hasInfix "id = \"task.ops.health\";" source;
assert pkgs.lib.hasInfix "id = \"task.ops.ready\";" source;
assert pkgs.lib.hasInfix "checking postgres health" source;
assert pkgs.lib.hasInfix "checking postgres readiness" source;
assert pkgs.lib.hasInfix "checking nginx health" source;
assert pkgs.lib.hasInfix "checking nginx readiness" source;
assert pkgs.lib.hasInfix "checking reth health" source;
assert pkgs.lib.hasInfix "checking reth readiness" source;
assert pkgs.lib.hasInfix "checking helios health" source;
assert pkgs.lib.hasInfix "checking helios readiness" source;
assert pkgs.lib.hasInfix "\"method\":\"web3_clientVersion\"" source;
assert pkgs.lib.hasInfix "\"method\":\"eth_chainId\"" source;
assert pkgs.lib.hasInfix "\"method\":\"eth_blockNumber\"" source;
pkgs.runCommand "operations-contract" { } ''
  echo "OK: operations health/readiness contract markers are stable" > "$out"
''
