# Reth Module Notes

## Configure in `nixfied/project/conf.nix`

```nix
modules.reth = {
  enable = true;
  portKeyHttp = "rethHttp";
  portKeyWs = "rethWs";
  portKeyAuth = "rethAuth";
  dataDirName = "reth";
  network = "local";
  devMode = true;
  extraArgs = [ ];
};
```

## Relevant Port Keys

- `portKeyHttp` maps to `ports.rethHttp`.
- `portKeyWs` maps to `ports.rethWs`.
- `portKeyAuth` maps to `ports.rethAuth`.

## Command Surface

No dedicated Reth app namespace is exposed. Use model-generated operations:

- `nix run .#validate-env`
- `nix run .#ports`
- `nix run .#check-ports`
