# Reth runtime adapter
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
  inherit (helpers) loggingPrelude;
  config = import ./config.nix { inherit pkgs project; };
  lifecycle = import ./lifecycle.nix {
    inherit
      pkgs
      project
      slots
      config
      loggingPrelude
      ;
  };
  preStart = pkgs.writeShellScript "reth-pre-start" ''
    ${loggingPrelude}

    set -euo pipefail

    ${lifecycle.init}
    ${lifecycle.checkConfig}
    exec ${lifecycle.preflightStart}
  '';
  preStop = pkgs.writeShellScript "reth-pre-stop" ''
    ${loggingPrelude}

    set -euo pipefail
    :
  '';
in
{
  version = 1;
  operations = {
    inherit (lifecycle) init;
    pre-start = preStart;
    pre-stop = preStop;
    preflight-start = lifecycle.preflightStart;
    start = lifecycle.startLeaf;
    start-leaf = lifecycle.startLeaf;
    inherit (lifecycle) stop;
    inherit (lifecycle) restart;
    inherit (lifecycle) status;
    inherit (lifecycle) health;
    inherit (lifecycle) ready;
    check-config = lifecycle.checkConfig;
    full-start = lifecycle.fullStart;
    full-start-leaf = lifecycle.fullStartLeaf;
    full-start-test = lifecycle.fullStartTest;
    full-start-test-leaf = lifecycle.fullStartTestLeaf;
  };
}
