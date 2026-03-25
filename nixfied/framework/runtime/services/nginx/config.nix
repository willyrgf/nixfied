# Nginx module config defaults
{ pkgs, project }:

import ../service-config-builder.nix {
  inherit pkgs project;
  name = "nginx";
  defaults = cfg: {
    package = cfg.package or null;
    portKeyHttp = cfg.portKeyHttp or "http";
    portKeyHttps = cfg.portKeyHttps or "https";
    dataDirName = cfg.dataDirName or "nginx";
  };
}
