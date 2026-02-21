# Nginx Module Notes

## Configure in `nixfied/project/conf.nix`

```nix
services.nginx = {
  enable = true;
  ports.http = "http";
  ports.https = "https";
  dataDirName = "nginx";
  sources.nixpkgs.package = pkgs.nginx;
  defaultSource = "nixpkgs";
};
```

## Relevant Port Keys

- `ports.http` maps to `ports.http`.
- `ports.https` maps to `ports.https`.

## Command Surface

No dedicated Nginx app namespace is exposed. Use model-generated commands and CI workflows:

- `nix run .#ci -- --mode env --summary`
- `nix run .#ports`
- `nix run .#check-ports`
- `nix run .#health -- --service nginx`
- `nix run .#ready -- --service nginx`
