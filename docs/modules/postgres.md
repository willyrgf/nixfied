# PostgreSQL Module

## Enable and configure

```nix
modules.postgres = {
  enable = true;
  database = "app";
  testDatabase = "app_test";
  extensions = [ ];
  package = pkgs.postgresql_16;
  portKey = "postgres";
  dataDirName = "postgres";
  extraConfig = "";
  envConfigs = {
    dev = { };
    prod = { };
    test = { };
  };
  migrations = {
    dir = "migrations";
    command = "";
    sourceDatabase = null;
  };
};
```

## Public apps

Generated app namespace:

```text
svc::postgres::<operation>
```

Primary operations:
- Lifecycle: `init`, `start`, `stop`, `restart`, `status`, `health`, `ready`, `check-config`, `full-start`, `full-start-test`, `list-instances`
- Database/migrations: `setup-db`, `ready-test`, `test-migrations`, `shell`
- Backups: `backup`, `restore`, `list-backups`, `verify-backup`, `cleanup-backups`
- Port management: `check-port`, `kill-port`
- Observability: `log`, `events`

Hook-only operation (not exposed as app):
- `ensure-migration-tested`

## Hooks

Hooks are exported from service contract operations (examples):
- `SVC_POSTGRES_START`
- `SVC_POSTGRES_STOP`
- `SVC_POSTGRES_READY`
- `SVC_POSTGRES_SETUP_DB`
- `SVC_POSTGRES_TEST_MIGRATIONS`
- `SVC_POSTGRES_ENSURE_MIGRATION_TESTED`
- `SVC_POSTGRES_LOG`
- `SVC_POSTGRES_EVENTS`

## Example usage

```bash
PROJECT_ENV=dev NIX_ENV=0 nix run .#svc::postgres::full-start
PROJECT_ENV=dev NIX_ENV=0 nix run .#svc::postgres::setup-db
PROJECT_ENV=dev NIX_ENV=0 nix run .#svc::postgres::backup -- nightly
PROJECT_ENV=dev NIX_ENV=0 nix run .#svc::postgres::shell -- -c "select 1;"
PROJECT_ENV=dev NIX_ENV=0 nix run .#svc::postgres::events -- --limit 50
```
