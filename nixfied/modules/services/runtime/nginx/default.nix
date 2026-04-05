# nginx runtime adapter
{
  pkgs,
  project,
  slots,
}:

let
  summary = import ../../../../framework/runtime/helpers/summary.nix { inherit pkgs project; };
  helpers = import ../../../../framework/runtime/helpers/helpers.nix {
    inherit pkgs project;
    inherit (summary) summaryParser;
  };
  loggingPrelude = helpers.loggingPrelude;
  config = import ./config.nix { inherit pkgs project; };
  templates = import ./templates.nix {
    inherit pkgs;
    package = config.package or pkgs.nginx;
  };
  lifecycle = import ./lifecycle.nix {
    inherit
      pkgs
      project
      slots
      config
      templates
      loggingPrelude
      ;
  };
  siteMgmt = import ./site-management.nix {
    inherit
      pkgs
      project
      slots
      config
      templates
      lifecycle
      loggingPrelude
      ;
  };
  ssl = import ./ssl.nix {
    inherit
      pkgs
      project
      slots
      config
      lifecycle
      loggingPrelude
      ;
  };
  preStart = pkgs.writeShellScript "nginx-pre-start" ''
    ${loggingPrelude}

    set -euo pipefail

    ${lifecycle.init}
    ${lifecycle.checkConfig}
    exec ${lifecycle.preflightStart}
  '';
  preStop = pkgs.writeShellScript "nginx-pre-stop" ''
    ${loggingPrelude}

    set -euo pipefail
    :
  '';
in
{
  version = 1;
  operations = {
    init = lifecycle.init;
    pre-start = preStart;
    pre-stop = preStop;
    preflight-start = lifecycle.preflightStart;
    start = lifecycle.startLeaf;
    start-leaf = lifecycle.startLeaf;
    stop = lifecycle.stop;
    restart = lifecycle.restart;
    status = lifecycle.status;
    health = lifecycle.health;
    ready = lifecycle.ready;
    check-config = lifecycle.checkConfig;
    reload = lifecycle.reload;
    full-start = lifecycle.fullStart;
    full-start-leaf = lifecycle.fullStartLeaf;
    full-start-test = lifecycle.fullStartTest;
    full-start-test-leaf = lifecycle.fullStartTestLeaf;
    list-instances = lifecycle.listInstances;
    site-proxy = siteMgmt.writeProxySite;
    site-static = siteMgmt.writeStaticSite;
    site-add = siteMgmt.addSite;
    site-remove = siteMgmt.removeSite;
    site-enable = siteMgmt.enableSite;
    site-disable = siteMgmt.disableSite;
    site-list = siteMgmt.listSites;
    cert-obtain = ssl.obtainCert;
    cert-renew = ssl.renewCerts;
    cert-status = ssl.certStatus;
  };
}
