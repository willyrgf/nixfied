# MinIO Module

## Enable and configure

```nix
modules.minio = {
  enable = true;
  package = pkgs.minio;
  clientPackage = pkgs.minio-client;
  portKeyApi = "minioApi";
  portKeyConsole = "minioConsole";
  dataDirName = "minio";
  rootUser = "minioadmin";
  rootPassword = "minioadmin";
  browser = true;
};
```

## Public apps

Generated app namespace:

```text
svc::minio::<operation>
```

Primary operations:
- Lifecycle: `init`, `start`, `stop`, `restart`, `status`, `health`, `ready`, `check-config`, `full-start`, `full-start-test`
- Bucket/admin: `export-s3-env`, `bucket-ensure`, `bucket-create`, `bucket-delete`, `bucket-list`, `policy-apply`
- Observability: `log`, `events`

## Hooks

Examples:
- `SVC_MINIO_START`
- `SVC_MINIO_READY`
- `SVC_MINIO_EXPORT_S3_ENV`
- `SVC_MINIO_BUCKET_LIST`
- `SVC_MINIO_POLICY_APPLY`
- `SVC_MINIO_LOG`
- `SVC_MINIO_EVENTS`

## Example usage

```bash
PROJECT_ENV=dev NIX_ENV=0 nix run .#svc::minio::full-start
PROJECT_ENV=dev NIX_ENV=0 nix run .#svc::minio::bucket-ensure -- artifacts
PROJECT_ENV=dev NIX_ENV=0 nix run .#svc::minio::export-s3-env -- artifacts local us-east-1
PROJECT_ENV=dev NIX_ENV=0 nix run .#svc::minio::bucket-list
```
