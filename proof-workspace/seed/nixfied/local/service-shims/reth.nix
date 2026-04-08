{
  pkgs,
  project,
  slots,
}:
let
  common = import ./common.nix {
    inherit
      (pkgs) lib
      ;
    inherit
      pkgs
      project
      slots
      ;
  };
  base = common.mkBaseOperations {
    serviceName = "reth";
    displayName = "Reth";
    dataDirName = "reth";
    logFileName = "reth.log";
    pidFileName = "reth.pid";
  };
in
{
  version = 1;
  operations = base.operations;
}
