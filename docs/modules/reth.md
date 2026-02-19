# Reth Module Notes

## Configure in `conf.nix`

```nix
modules.reth = {
  enable = true;
  portKeyHttp = "rethHttp";
  portKeyWs = "rethWs";
  portKeyAuth = "rethAuth";
};
```

## Command surface

No dedicated Reth command namespace is currently exposed as a standalone app surface.
Use `nix run .#help` to see the current model-generated commands.
