# Helios Module Notes

## Configure in `nixfied/project/conf.nix`

```nix
services.helios = {
  enable = true;
  ports.rpc = "heliosRpc";
  ports.executionRpc = "rethHttp";
  dataDirName = "helios";
  network = "local";
  executionRpcUrl = "";
  consensusRpcUrl = "";
  checkpoint = "";
  extraArgs = [ ];
  sources = {
    stable.package = pkgs.callPackage ./nixfied/project/sources/helios-stable.nix { };
    patched.package = pkgs.callPackage ./nixfied/project/sources/helios-patched.nix { };
  };
  defaultSource = "stable";
};
```

## Relevant Port Keys

- `ports.rpc` maps to `ports.heliosRpc`.
- `ports.executionRpc` should point at an execution-layer RPC key (default: `rethHttp`).

## Command Surface

No dedicated Helios app namespace is exposed. Use model-generated operations:

- `nix run .#validate-env`
- `nix run .#ports`
- `nix run .#check-ports`
- `nix run .#health -- --service helios --source stable`
- `nix run .#ready -- --service helios --source patched`
