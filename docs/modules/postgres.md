# Postgres Module Notes

## Configure in `conf.nix`

```nix
modules.postgres = {
  enable = true;
  database = "app";
  portKey = "postgres";
};
```

## Command surface

No dedicated Postgres command namespace is currently exposed as a standalone app surface.
Use `nix run .#help` to see the current model-generated commands.
