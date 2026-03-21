{ lib, ... }:
{
  nixfied.services.helios = {
    enable = lib.mkForce true;
    sourceKeys = lib.mkForce [ "poison" ];
    defaultSource = lib.mkForce "poison";
    sources.poison.packageFactory = ./poison-package.nix;
  };
}
