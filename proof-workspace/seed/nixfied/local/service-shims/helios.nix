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
    serviceName = "helios";
    displayName = "Helios";
    dataDirName = "helios";
    logFileName = "helios.log";
    pidFileName = "helios.pid";
  };
in
{
  version = 1;
  inherit (base) operations;
}
