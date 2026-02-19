# Nginx Module Notes

## Configure in `conf.nix`

```nix
modules.nginx = {
  enable = true;
  portKeyHttp = "http";
  portKeyHttps = "https";
};
```

## Command surface

No dedicated Nginx command namespace is currently exposed as a standalone app surface.
Use `nix run .#help` to see the current model-generated commands.
