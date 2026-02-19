# Nginx Module Notes

## Configure in `nixfied/project/conf.nix`

```nix
modules.nginx = {
  enable = true;
  portKeyHttp = "http";
  portKeyHttps = "https";
  dataDirName = "nginx";
};
```

## Relevant Port Keys

- `portKeyHttp` maps to `ports.http`.
- `portKeyHttps` maps to `ports.https`.

## Command Surface

No dedicated Nginx app namespace is exposed. Use model-generated commands and CI workflows:

- `nix run .#ci -- --mode env --summary`
- `nix run .#ports`
- `nix run .#check-ports`
