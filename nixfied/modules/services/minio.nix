{
  lib,
  config,
  ...
}:
let
  t = lib.types;
  cfg = config.nixfied.services.minio;
  probeLib = import ./probes.nix { inherit lib; };
  contractSchema = import ./contract-schema.nix { inherit lib; };
  operationContractBuilder = import ./operation-contract-builder.nix;
  sourceOptions = import ./source-options.nix { inherit lib; };
  sourceSpec = sourceOptions.mkSourceSpec {
    withClientPackage = true;
  };
  serviceDir = contractSchema.mkServiceDirExpr cfg.dataDirName;
in
{
  options.nixfied.services.minio = {
    enable = lib.mkOption {
      type = t.bool;
      default = false;
    };
    portKeyApi = lib.mkOption {
      type = t.str;
      default = "minioApi";
    };
    portKeyConsole = lib.mkOption {
      type = t.str;
      default = "minioConsole";
    };
    dataDirName = lib.mkOption {
      type = t.str;
      default = "minio";
    };
    rootUser = lib.mkOption {
      type = t.str;
      default = "minioadmin";
    };
    rootPassword = lib.mkOption {
      type = t.str;
      default = "minioadmin";
    };
    browser = lib.mkOption {
      type = t.bool;
      default = true;
    };
    sources = lib.mkOption {
      type = t.attrsOf sourceSpec;
      default = { };
    };
    sourceKeys = lib.mkOption {
      type = t.listOf t.str;
      default = [ ];
    };
    defaultSource = lib.mkOption {
      type = t.str;
      default = "";
    };
    probes = probeLib.probeOptions;
    contract = contractSchema.mkContractOption "Typed MinIO public contract.";
  };

  config.nixfied.services.minio.contract = {
    version = 1;
    service = "minio";
    summary = "MinIO service management API";
    details = "Public service contract for managing MinIO across dev/prod/test/ci.";
    ownerFile = "nixfied/modules/services/minio.nix";
    adapter = {
      version = 1;
      module = ../../framework/runtime/services/minio/default.nix;
    };
    profiles = [
      "dev"
      "prod"
      "test"
      "ci"
    ];
    artifacts = {
      apiPortVar = contractSchema.mkPortVarName cfg.portKeyApi;
      consolePortVar = contractSchema.mkPortVarName cfg.portKeyConsole;
      serviceDir = serviceDir;
      dataDir = serviceDir;
      logFile = "${serviceDir}/logs/minio.log";
      pidFile = "${serviceDir}/run/minio.pid";
    };
    runtimePrimitives = contractSchema.mkRuntimePrimitivesV1 config.nixfied.runtime;
    operations =
      (operationContractBuilder {
        displayName = "MinIO";
        extraOperations = {
          start = {
            runtimeOp = "start-leaf";
            preOps = [
              "init"
              "check-config"
              "preflight-start"
            ];
            summary = "Start MinIO server";
            details = "Starts MinIO with API and console listeners for the current slot and environment.";
          };

          export-s3-env = {
            runtimeOp = "export-s3-env";
            hook = "EXPORT_S3_ENV";
            summary = "Export S3-compatible environment from MinIO";
            details = "Prints shell exports for AWS/S3 variables targeting the current MinIO endpoint.";
            usage = [ "eval \"$(nix run .#svc::minio::export-s3-env -- <bucket> <prefix> <region>)\"" ];
          };

          bucket-ensure = {
            runtimeOp = "bucket-ensure";
            hook = "BUCKET_ENSURE";
            summary = "Ensure MinIO bucket exists";
            details = "Creates bucket when missing and succeeds when already present.";
            usage = [ "nix run .#svc::minio::bucket-ensure -- <bucket>" ];
          };

          bucket-create = {
            runtimeOp = "bucket-create";
            hook = "BUCKET_CREATE";
            summary = "Create MinIO bucket";
            details = "Creates a bucket in the running MinIO instance.";
            usage = [ "nix run .#svc::minio::bucket-create -- <bucket>" ];
          };

          bucket-delete = {
            runtimeOp = "bucket-delete";
            hook = "BUCKET_DELETE";
            summary = "Delete MinIO bucket";
            details = "Deletes a bucket from the running MinIO instance.";
            usage = [ "nix run .#svc::minio::bucket-delete -- <bucket>" ];
          };

          bucket-list = {
            runtimeOp = "bucket-list";
            hook = "BUCKET_LIST";
            summary = "List MinIO buckets";
            details = "Lists buckets from the running MinIO instance.";
          };

          policy-apply = {
            runtimeOp = "policy-apply";
            hook = "POLICY_APPLY";
            summary = "Apply MinIO bucket policy";
            details = "Applies a JSON policy file to a MinIO bucket.";
            usage = [ "nix run .#svc::minio::policy-apply -- <bucket> <policy-file>" ];
          };
        };
      })
      // contractSchema.mkObservabilityOperations {
        service = "minio";
        summaryName = "MinIO";
      };
  };
}
