# Reth module config defaults
{ pkgs, project }:

import ../service-config-builder.nix {
  inherit pkgs project;
  name = "reth";
  defaults = cfg: {
    package = cfg.package or null;
    portKeyHttp = cfg.portKeyHttp or "rethHttp";
    portKeyWs = cfg.portKeyWs or "rethWs";
    portKeyAuth = cfg.portKeyAuth or "rethAuth";
    dataDirName = cfg.dataDirName or "reth";
    network = cfg.network or "local";
    devMode = cfg.devMode or false;
    extraArgs = cfg.extraArgs or [ ];
  };
}
