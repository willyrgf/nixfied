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
    pinned.package = pkgs.callPackage ./nixfied/project/sources/helios-pinned.nix { };
    nixpkgs.package = pkgs.helios;
  };
  sourceKinds = {
    pinned = "real";
    nixpkgs = "real";
    shim = "shim";
  };
  defaultSource = "pinned";
  readiness = {
    profile = "strict";
    requireNotSyncing = true;
    disallowSourceKinds = [ "shim" "unknown" ];
  };
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
- `nix run .#health -- --service helios --source pinned`
- `nix run .#ready -- --service helios --source nixpkgs`

In strict readiness mode, `ready` rejects disallowed source kinds and requires `eth_syncing=false`.
