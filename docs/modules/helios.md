# Helios Module Notes

## Configure in `nixfied/project/conf.nix`

```nix
modules.helios = {
  enable = true;
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

## Relevant Port Keys

- `portKeyRpc` maps to `ports.heliosRpc`.
- `executionRpcPortKey` should point at an execution-layer RPC key (default: `rethHttp`).

## Command Surface

No dedicated Helios app namespace is exposed. Use model-generated operations:

- `nix run .#validate-env`
- `nix run .#ports`
- `nix run .#check-ports`
