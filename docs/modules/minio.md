# MinIO Module Notes

## Configure in `nixfied/project/conf.nix`

```nix
modules.minio = {
  enable = true;
  portKeyApi = "minioApi";
  portKeyConsole = "minioConsole";
  dataDirName = "minio";
  rootUser = "minioadmin";
  rootPassword = "minioadmin";
  browser = true;
};
```

## Relevant Port Keys

- `portKeyApi` maps to `ports.minioApi`.
- `portKeyConsole` maps to `ports.minioConsole`.

## Command Surface

No dedicated MinIO app namespace is exposed. Use model-generated operations:

- `nix run .#validate-env`
- `nix run .#ports`
- `nix run .#check-ports`
