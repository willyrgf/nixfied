# Postgres Module Notes

## Configure in `nixfied/project/conf.nix`

```nix
modules.postgres = {
  enable = true;
  database = "app";
  testDatabase = "app_test";
  extensions = [ ];
  portKey = "postgres";
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
};
```

## Relevant Port Keys

- `portKey = "postgres"` maps to `ports.postgres` in `conf.nix`.

## Command Surface

No dedicated Postgres app namespace is exposed. Use model-generated commands:

- `nix run .#validate-env`
- `nix run .#ports`
- `nix run .#check-ports`
