# Nginx Module

## Enable and configure

```nix
modules.nginx = {
  enable = true;
  portKeyHttp = "http";
  portKeyHttps = "https";
  dataDirName = "nginx";
};
```

## Public apps

Generated app namespace:

```text
svc::nginx::<operation>
```

Primary operations:
- Lifecycle: `init`, `start`, `stop`, `restart`, `status`, `health`, `ready`, `reload`, `check-config`, `list-instances`
- Site management: `site-add`, `site-remove`, `site-list`, `site-enable`, `site-disable`
- TLS: `cert-obtain`, `cert-renew`, `cert-status`
- Observability: `log`, `events`

Hook-only operations (not exposed as apps):
- `site-proxy`
- `site-static`

## Hooks

Examples:
- `SVC_NGINX_START`
- `SVC_NGINX_RELOAD`
- `SVC_NGINX_SITE_ADD`
- `SVC_NGINX_CERT_OBTAIN`
- `SVC_NGINX_LOG`
- `SVC_NGINX_EVENTS`

## Example usage

```bash
PROJECT_ENV=dev NIX_ENV=0 nix run .#svc::nginx::init
PROJECT_ENV=dev NIX_ENV=0 nix run .#svc::nginx::site-add -- example.localhost 127.0.0.1 3000
PROJECT_ENV=dev NIX_ENV=0 nix run .#svc::nginx::start
PROJECT_ENV=dev NIX_ENV=0 nix run .#svc::nginx::site-list
PROJECT_ENV=dev NIX_ENV=0 nix run .#svc::nginx::log -- --follow
```
