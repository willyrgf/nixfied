# Reth Module

## Enable and configure

```nix
modules.reth = {
  enable = true;
  package = pkgs.reth;
  portKeyHttp = "rethHttp";
  portKeyWs = "rethWs";
  portKeyAuth = "rethAuth";
  dataDirName = "reth";
  network = "local";
  devMode = true;
  extraArgs = [ ];
};
```

## Public apps

Generated app namespace:

```text
svc::reth::<operation>
```

Primary operations:
- `init`, `start`, `stop`, `restart`, `status`, `health`, `ready`, `check-config`, `full-start`, `full-start-test`
- Observability: `log`, `events`

## Hooks

Examples:
- `SVC_RETH_START`
- `SVC_RETH_READY`
- `SVC_RETH_CHECK_CONFIG`
- `SVC_RETH_LOG`
- `SVC_RETH_EVENTS`

## Example usage

```bash
PROJECT_ENV=dev NIX_ENV=0 nix run .#svc::reth::full-start
PROJECT_ENV=dev NIX_ENV=0 nix run .#svc::reth::health
PROJECT_ENV=dev NIX_ENV=0 nix run .#svc::reth::events -- --limit 20
```
