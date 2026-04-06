{ lib, ... }:
{
  # Deliberately poison Helios source selection so any accidental evaluation of
  # the Helios branch fails immediately and deterministically.
  nixfied.services.helios = {
    sources = lib.mkForce {
      poison.package = throw "helios evaluated unexpectedly";
    };
    sourceKeys = lib.mkForce [ "poison" ];
    defaultSource = lib.mkForce "poison";
    sourceKinds = lib.mkForce { poison = "poison"; };
  };
}
