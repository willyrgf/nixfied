{ lib }:
let
  archFromSystem = system: builtins.elemAt (lib.splitString "-" system) 0;
  osFromSystem =
    system:
    if lib.hasSuffix "-darwin" system then
      "darwin"
    else if lib.hasSuffix "-linux" system then
      "linux"
    else
      throw "unsupported M0 target system: ${system}";
in
{
  fromSystem =
    system:
    {
      inherit system;
      os = osFromSystem system;
      arch = archFromSystem system;
      closureSystem = system;
      requiredRuntimeCapabilities = {
        processGroup = true;
        tcpPortOwnership = true;
        sqliteWal = true;
      };
    };
}
