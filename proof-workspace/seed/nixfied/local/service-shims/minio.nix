{
  pkgs,
  project,
  slots,
}:
let
  common = import ./common.nix {
    inherit
      (pkgs) lib
      ;
    inherit
      pkgs
      project
      slots
      ;
  };
  base = common.mkBaseOperations {
    serviceName = "minio";
    displayName = "MinIO";
    dataDirName = "minio";
    logFileName = "minio.log";
    pidFileName = "minio.pid";
  };
  inherit (base) mkCommand;
in
{
  version = 1;
  operations = base.operations // {
    "export-s3-env" = mkCommand "export-s3-env" ''
      require_running
      bucket_name="''${1:-proof-bucket}"
      prefix_name="''${2:-proof-prefix}"
      region_name="''${3:-us-east-1}"
      append_operation "export-s3-env:$service_name"
      printf 'export AWS_ACCESS_KEY_ID=%s\n' "$minio_root_user"
      printf 'export AWS_SECRET_ACCESS_KEY=%s\n' "$minio_root_password"
      printf 'export AWS_DEFAULT_REGION=%s\n' "$region_name"
      printf 'export AWS_ENDPOINT_URL=http://127.0.0.1:%s\n' "$port_api"
      printf 'export AWS_BUCKET=%s\n' "$bucket_name"
      printf 'export AWS_PREFIX=%s\n' "$prefix_name"
    '';

    "bucket-create" = mkCommand "bucket-create" ''
      require_running
      bucket_name="''${1:-}"
      if [ -z "$bucket_name" ]; then
        echo "ERROR: usage: bucket-create <bucket>"
        exit 2
      fi
      mkdir -p "$bucket_dir/$bucket_name"
      append_operation "bucket-create:$service_name"
      echo "OK: service=$service_name bucket=$bucket_name created=1"
    '';

    "bucket-ensure" = mkCommand "bucket-ensure" ''
      require_running
      bucket_name="''${1:-}"
      if [ -z "$bucket_name" ]; then
        echo "ERROR: usage: bucket-ensure <bucket>"
        exit 2
      fi
      mkdir -p "$bucket_dir/$bucket_name"
      append_operation "bucket-ensure:$service_name"
      echo "OK: service=$service_name bucket=$bucket_name ensured=1"
    '';

    "bucket-delete" = mkCommand "bucket-delete" ''
      require_running
      bucket_name="''${1:-}"
      if [ -z "$bucket_name" ]; then
        echo "ERROR: usage: bucket-delete <bucket>"
        exit 2
      fi
      rm -rf "$bucket_dir/$bucket_name"
      append_operation "bucket-delete:$service_name"
      echo "OK: service=$service_name bucket=$bucket_name deleted=1"
    '';

    "bucket-list" = mkCommand "bucket-list" ''
      require_running
      append_operation "bucket-list:$service_name"
      found=0
      for bucket_path in "$bucket_dir"/*; do
        if [ ! -e "$bucket_path" ]; then
          continue
        fi
        found=1
        echo "INFO: bucket=$(basename "$bucket_path")"
      done
      if [ "$found" -eq 0 ]; then
        echo "INFO: bucket=none"
      fi
    '';

    "policy-apply" = mkCommand "policy-apply" ''
      require_running
      bucket_name="''${1:-}"
      policy_file="''${2:-}"
      if [ -z "$bucket_name" ] || [ -z "$policy_file" ]; then
        echo "ERROR: usage: policy-apply <bucket> <policy-file>"
        exit 2
      fi
      if [ ! -f "$policy_file" ]; then
        echo "ERROR: policy missing path=$policy_file"
        exit 1
      fi
      mkdir -p "$policy_dir"
      cp "$policy_file" "$policy_dir/$bucket_name.json"
      append_operation "policy-apply:$service_name"
      echo "OK: service=$service_name bucket=$bucket_name policy=$policy_file"
    '';
  };
}
