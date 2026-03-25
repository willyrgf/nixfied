# MinIO module config defaults
{ pkgs, project }:

import ../service-config-builder.nix {
  inherit pkgs project;
  name = "minio";
  defaults = cfg: {
    package = cfg.package or null;
    clientPackage = cfg.clientPackage or null;
    rootUser = cfg.rootUser or "minioadmin";
    rootPassword = cfg.rootPassword or "minioadmin";
    portKeyApi = cfg.portKeyApi or "minioApi";
    portKeyConsole = cfg.portKeyConsole or "minioConsole";
    dataDirName = cfg.dataDirName or "minio";
    browser = cfg.browser or true;
  };
}
