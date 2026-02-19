# Helios Module Notes

## Configure in `conf.nix`

```nix
modules.helios = {
  enable = true;
  portKeyRpc = "heliosRpc";
  executionRpcPortKey = "rethHttp";
};
```

## Command surface

No dedicated Helios command namespace is currently exposed as a standalone app surface.
Use `nix run .#help` to see the current model-generated commands.
