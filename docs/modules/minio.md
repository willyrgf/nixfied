# MinIO Module Notes

## Configure in `nixfied/project/conf.nix`

```nix
services.minio = {
  enable = true;
  ports.api = "minioApi";
  ports.console = "minioConsole";
  dataDirName = "minio";
  rootUser = "minioadmin";
  rootPassword = "minioadmin";
  browser = true;
  sources.nixpkgs = {
    package = pkgs.minio;
    clientPackage = pkgs.minio-client;
  };
  defaultSource = "nixpkgs";
};
```

## Relevant Port Keys

- `ports.api` maps to `ports.minioApi`.
- `ports.console` maps to `ports.minioConsole`.

## Command Surface

No dedicated MinIO app namespace is exposed. Use model-generated operations:

- `nix run .#validate-env`
- `nix run .#ports`
- `nix run .#check-ports`
- `nix run .#health -- --service minio`
- `nix run .#ready -- --service minio`
