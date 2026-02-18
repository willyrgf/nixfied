# Helios Module

## Enable and configure

```nix
modules.helios = {
  enable = true;
  package = pkgs.callPackage ../.framework/helios/package.nix { };
  portKeyRpc = "heliosRpc";
  dataDirName = "helios";
  network = "local";
  executionRpcPortKey = "rethHttp";
  executionRpcUrl = "";
  consensusRpcUrl = "";
  checkpoint = "";
  extraArgs = [ ];
};
```

## Public apps

Generated app namespace:

```text
svc::helios::<operation>
```

Primary operations:
- `init`, `start`, `stop`, `restart`, `status`, `health`, `ready`, `check-config`, `full-start`, `full-start-test`
- Observability: `log`, `events`

## Readiness tunables

`svc::helios::ready` supports:
- `HELIOS_READY_TIMEOUT_SECS` (default `300`)
- `HELIOS_READY_INTERVAL_SECS` (default `1`)

## Hooks

Examples:
- `SVC_HELIOS_START`
- `SVC_HELIOS_READY`
- `SVC_HELIOS_CHECK_CONFIG`
- `SVC_HELIOS_LOG`
- `SVC_HELIOS_EVENTS`

## Example usage

```bash
PROJECT_ENV=dev NIX_ENV=0 nix run .#svc::helios::full-start
PROJECT_ENV=dev NIX_ENV=0 nix run .#svc::helios::ready
PROJECT_ENV=dev NIX_ENV=0 nix run .#svc::helios::log -- --lines 200
```
