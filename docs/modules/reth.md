# Reth Module Notes

## Configure in `nixfied/project/conf.nix`

```nix
services.reth = {
  enable = true;
  ports.http = "rethHttp";
  ports.ws = "rethWs";
  ports.auth = "rethAuth";
  dataDirName = "reth";
  network = "local";
  devMode = true;
  extraArgs = [ ];
  sources.nixpkgs.package = pkgs.reth;
  defaultSource = "nixpkgs";
};
```

## Relevant Port Keys

- `ports.http` maps to `ports.rethHttp`.
- `ports.ws` maps to `ports.rethWs`.
- `ports.auth` maps to `ports.rethAuth`.

## Command Surface

No dedicated Reth app namespace is exposed. Use model-generated operations:

- `nix run .#validate-env`
- `nix run .#ports`
- `nix run .#check-ports`
- `nix run .#health -- --service reth`
- `nix run .#ready -- --service reth`
