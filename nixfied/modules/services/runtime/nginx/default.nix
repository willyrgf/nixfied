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
in
{
  version = 1;
  operations = {
    init = lifecycle.init;
    preflight-start = lifecycle.preflightStart;
    preflightStart = lifecycle.preflightStart;
    start = lifecycle.start;
    start-leaf = lifecycle.startLeaf;
    startLeaf = lifecycle.startLeaf;
    stop = lifecycle.stop;
    restart = lifecycle.restart;
    status = lifecycle.status;
    health = lifecycle.health;
    ready = lifecycle.ready;
    check-config = lifecycle.checkConfig;
    checkConfig = lifecycle.checkConfig;
    reload = lifecycle.reload;
    full-start = lifecycle.fullStart;
    full-start-leaf = lifecycle.fullStartLeaf;
    fullStart = lifecycle.fullStart;
    fullStartLeaf = lifecycle.fullStartLeaf;
    full-start-test = lifecycle.fullStartTest;
    full-start-test-leaf = lifecycle.fullStartTestLeaf;
    fullStartTest = lifecycle.fullStartTest;
    fullStartTestLeaf = lifecycle.fullStartTestLeaf;
    list-instances = lifecycle.listInstances;
    listInstances = lifecycle.listInstances;
    site-proxy = siteMgmt.writeProxySite;
    site-static = siteMgmt.writeStaticSite;
    site-add = siteMgmt.addSite;
    site-remove = siteMgmt.removeSite;
    site-enable = siteMgmt.enableSite;
    site-disable = siteMgmt.disableSite;
    site-list = siteMgmt.listSites;
    siteProxy = siteMgmt.writeProxySite;
    siteStatic = siteMgmt.writeStaticSite;
    siteAdd = siteMgmt.addSite;
    siteRemove = siteMgmt.removeSite;
    siteEnable = siteMgmt.enableSite;
    siteDisable = siteMgmt.disableSite;
    siteList = siteMgmt.listSites;
    cert-obtain = ssl.obtainCert;
    cert-renew = ssl.renewCerts;
    cert-status = ssl.certStatus;
    certObtain = ssl.obtainCert;
    certRenew = ssl.renewCerts;
    certStatus = ssl.certStatus;
  };
}
