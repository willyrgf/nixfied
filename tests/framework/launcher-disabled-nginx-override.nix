{ lib, ... }:
{
  nixfied.services.nginx.enable = lib.mkForce false;
  nixfied.services.nginx.sources = lib.mkForce {
    poison.package = throw "nginx evaluated unexpectedly";
  };
  nixfied.services.nginx.sourceKeys = lib.mkForce [ "poison" ];
  nixfied.services.nginx.defaultSource = lib.mkForce "poison";
}
