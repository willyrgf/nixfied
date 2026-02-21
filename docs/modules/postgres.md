# Postgres Module Notes

## Configure in `nixfied/project/conf.nix`

```nix
services.postgres = {
  enable = true;
  ports.primary = "postgres";
  database = "app";
  testDatabase = "app_test";
  extensions = [ ];
  dataDirName = "postgres";
  extraConfig = "";
  envConfigs = {
    dev = { };
    test = { };
    prod = { };
  };
  migrations = {
    dir = "migrations";
    command = "";
    sourceDatabase = null;
  };
  sources.nixpkgs.package = pkgs.postgresql_16;
  defaultSource = "nixpkgs";
};
```

## Relevant Port Keys

- `ports.primary = "postgres"` maps to `ports.postgres` in `conf.nix`.

## Command Surface

No dedicated Postgres app namespace is exposed. Use model-generated commands:

- `nix run .#validate-env`
- `nix run .#ports`
- `nix run .#check-ports`
- `nix run .#health -- --service postgres`
- `nix run .#ready -- --service postgres`
