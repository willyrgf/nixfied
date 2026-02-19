# MinIO Module Notes

## Configure in `conf.nix`

```nix
modules.minio = {
  enable = true;
  portKeyApi = "minioApi";
  portKeyConsole = "minioConsole";
};
```

## Command surface

No dedicated MinIO command namespace is currently exposed as a standalone app surface.
Use `nix run .#help` to see the current model-generated commands.
