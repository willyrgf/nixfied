{ lib, ... }:
{
  nixfied.services.nginx = {
    enable = lib.mkForce false;
    sources = lib.mkForce {
      poison.package = throw "nginx evaluated unexpectedly";
    };
    sourceKeys = lib.mkForce [ "poison" ];
    defaultSource = lib.mkForce "poison";
  };
}
