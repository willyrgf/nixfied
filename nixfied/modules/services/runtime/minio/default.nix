# MinIO runtime adapter
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
  lifecycle = import ./lifecycle.nix {
    inherit
      pkgs
      project
      slots
      config
      loggingPrelude
      ;
  };
  bucketMgmt = import ./bucket-management.nix {
    inherit
      pkgs
      project
      slots
      config
      loggingPrelude
      ;
  };
  preStart = pkgs.writeShellScript "minio-pre-start" ''
    ${loggingPrelude}

    set -euo pipefail

    ${lifecycle.init}
    ${lifecycle.checkConfig}
    exec ${lifecycle.preflightStart}
  '';
  preStop = pkgs.writeShellScript "minio-pre-stop" ''
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
    full-start = lifecycle.fullStart;
    full-start-leaf = lifecycle.fullStartLeaf;
    full-start-test = lifecycle.fullStartTest;
    full-start-test-leaf = lifecycle.fullStartTestLeaf;
    export-s3-env = lifecycle.exportS3Env;
    bucket-create = bucketMgmt.bucketCreate;
    bucket-ensure = bucketMgmt.bucketEnsure;
    bucket-delete = bucketMgmt.bucketDelete;
    bucket-list = bucketMgmt.bucketList;
    policy-apply = bucketMgmt.policyApply;
  };
}
