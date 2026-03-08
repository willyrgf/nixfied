{ pkgs }:
let
  source = builtins.readFile ../../nixfied/.framework/nginx/site-management.nix;
in
assert pkgs.lib.hasInfix "resolvedTemplates.siteProxyTemplate" source;
assert pkgs.lib.hasInfix "resolvedTemplates.siteStaticTemplate" source;
assert pkgs.lib.hasInfix "render_site_template()" source;
assert !(pkgs.lib.hasInfix "render_proxy_site_config()" source);
assert !(pkgs.lib.hasInfix "render_static_site_config()" source);
pkgs.runCommand "nginx-site-management-contract" { } ''
  echo "OK: nginx site management renders from compiled templates" > "$out"
''
