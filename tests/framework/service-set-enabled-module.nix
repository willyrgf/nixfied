{ lib, ... }:
{
  nixfied.services.postgres.enable = lib.mkForce true;
  nixfied.services.minio.enable = lib.mkForce true;
}
