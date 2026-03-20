{ lib, ... }:
{
  nixfied.services.helios.sources = lib.mkForce {
    poison.package = throw "helios evaluated unexpectedly";
  };
  nixfied.services.helios.sourceKeys = lib.mkForce [ "poison" ];
  nixfied.services.helios.defaultSource = lib.mkForce "poison";
  nixfied.services.helios.sourceKinds = lib.mkForce { poison = "poison"; };
}
