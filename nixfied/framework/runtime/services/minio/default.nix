# MinIO module aggregator
{
  pkgs,
  project,
  slots,
}:

let
  summary = import ../../helpers/summary.nix { inherit pkgs project; };
  helpers = import ../../helpers/helpers.nix {
    inherit pkgs project;
    inherit (summary) summaryParser;
  };
  loggingPrelude = helpers.loggingPrelude;
  serviceModule = import ../../helpers/service-module.nix { inherit pkgs project slots; };
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

  operations = import ../service-operations-builder.nix {
    displayName = "MinIO";
    inherit lifecycle;
    extraOperations = {
      # Override start details
      start = {
        script = lifecycle.startLeaf;
        preOps = [
          "init"
          "check-config"
          "preflight-start"
        ];
        summary = "Start MinIO server";
        details = "Starts MinIO with API and console listeners for the current slot and environment.";
      };
      export-s3-env = {
        script = lifecycle.exportS3Env;
        hook = "EXPORT_S3_ENV";
        summary = "Export S3-compatible environment from MinIO";
        details = "Prints shell exports for AWS/S3 variables targeting the current MinIO endpoint.";
        usage = [ "eval \"$(nix run .#svc::minio::export-s3-env -- <bucket> <prefix> <region>)\"" ];
      };
      bucket-ensure = {
        script = bucketMgmt.bucketEnsure;
        hook = "BUCKET_ENSURE";
        summary = "Ensure MinIO bucket exists";
        details = "Creates bucket when missing and succeeds when already present.";
        usage = [ "nix run .#svc::minio::bucket-ensure -- <bucket>" ];
      };
      bucket-create = {
        script = bucketMgmt.bucketCreate;
        hook = "BUCKET_CREATE";
        summary = "Create MinIO bucket";
        details = "Creates a bucket in the running MinIO instance.";
        usage = [ "nix run .#svc::minio::bucket-create -- <bucket>" ];
      };
      bucket-delete = {
        script = bucketMgmt.bucketDelete;
        hook = "BUCKET_DELETE";
        summary = "Delete MinIO bucket";
        details = "Deletes a bucket from the running MinIO instance.";
        usage = [ "nix run .#svc::minio::bucket-delete -- <bucket>" ];
      };
      bucket-list = {
        script = bucketMgmt.bucketList;
        hook = "BUCKET_LIST";
        summary = "List MinIO buckets";
        details = "Lists buckets from the running MinIO instance.";
      };
      policy-apply = {
        script = bucketMgmt.policyApply;
        hook = "POLICY_APPLY";
        summary = "Apply MinIO bucket policy";
        details = "Applies a JSON policy file to a MinIO bucket.";
        usage = [ "nix run .#svc::minio::policy-apply -- <bucket> <policy-file>" ];
      };
    };
  };
in
serviceModule.mkServiceModule {
  service = "minio";
  summaryName = "MinIO";
  summary = "MinIO service management API";
  details = "Public service contract for managing MinIO across dev/prod/test/ci.";
  profiles = [
    "dev"
    "prod"
    "test"
    "ci"
  ];
  artifacts = {
    apiPortVar = slots.portVarName config.portKeyApi;
    consolePortVar = slots.portVarName config.portKeyConsole;
    serviceDir = slots.getServiceDir config.dataDirName;
    dataDir = slots.getServiceDir config.dataDirName;
    logFile = "${slots.getServiceDir config.dataDirName}/logs/minio.log";
    pidFile = "${slots.getServiceDir config.dataDirName}/run/minio.pid";
  };
  inherit
    config
    operations
    ;
  exported = {
    inherit (lifecycle)
      minio
      init
      start
      stop
      restart
      status
      health
      checkConfig
      ready
      fullStart
      fullStartTest
      exportS3Env
      ;

    inherit (bucketMgmt)
      bucketCreate
      bucketEnsure
      bucketDelete
      bucketList
      policyApply
      ;
  };
}
