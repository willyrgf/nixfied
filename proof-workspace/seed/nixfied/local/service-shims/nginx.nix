{
  pkgs,
  project,
  slots,
}:
let
  common = import ./common.nix {
    inherit
      (pkgs) lib
      ;
    inherit
      pkgs
      project
      slots
      ;
  };
  base = common.mkBaseOperations {
    serviceName = "nginx";
    displayName = "nginx";
    dataDirName = "nginx";
    logFileName = "error.log";
    pidFileName = "nginx.pid";
  };
  mkCommand = base.mkCommand;
in
{
  version = 1;
  operations = base.operations // {
    reload = mkCommand "reload" ''
      require_running
      append_operation "reload:$service_name"
      echo "OK: service=$service_name reload=complete"
    '';

    "list-instances" = mkCommand "list-instances" ''
      state="stopped"
      if is_running; then
        state="running"
      fi
      append_operation "list-instances:$service_name"
      echo "INFO: service=$service_name state=$state http=$port_http https=$port_https"
    '';

    "site-proxy" = mkCommand "site-proxy" ''
      domain="''${1:-}"
      upstream_host="''${2:-}"
      upstream_port="''${3:-}"
      if [ -z "$domain" ] || [ -z "$upstream_host" ] || [ -z "$upstream_port" ]; then
        echo "ERROR: usage: site-proxy <domain> <upstream-host> <upstream-port>"
        exit 2
      fi
      printf 'mode=proxy\nhost=%s\nport=%s\n' "$upstream_host" "$upstream_port" > "$site_dir/$domain.conf"
      ln -sf "../sites/$domain.conf" "$enabled_site_dir/$domain.conf"
      append_operation "site-proxy:$service_name"
      echo "OK: service=$service_name site=$domain mode=proxy"
    '';

    "site-static" = mkCommand "site-static" ''
      domain="''${1:-}"
      site_root="''${2:-}"
      if [ -z "$domain" ] || [ -z "$site_root" ]; then
        echo "ERROR: usage: site-static <domain> <site-root>"
        exit 2
      fi
      printf 'mode=static\nroot=%s\n' "$site_root" > "$site_dir/$domain.conf"
      ln -sf "../sites/$domain.conf" "$enabled_site_dir/$domain.conf"
      append_operation "site-static:$service_name"
      echo "OK: service=$service_name site=$domain mode=static"
    '';

    "site-add" = mkCommand "site-add" ''
      domain="''${1:-}"
      upstream_host="''${2:-}"
      upstream_port="''${3:-}"
      if [ -z "$domain" ] || [ -z "$upstream_host" ] || [ -z "$upstream_port" ]; then
        echo "ERROR: usage: site-add <domain> <upstream-host> <upstream-port>"
        exit 2
      fi
      printf 'mode=proxy\nhost=%s\nport=%s\n' "$upstream_host" "$upstream_port" > "$site_dir/$domain.conf"
      ln -sf "../sites/$domain.conf" "$enabled_site_dir/$domain.conf"
      append_operation "site-add:$service_name"
      echo "OK: service=$service_name site=$domain enabled=1"
    '';

    "site-remove" = mkCommand "site-remove" ''
      domain="''${1:-}"
      if [ -z "$domain" ]; then
        echo "ERROR: usage: site-remove <domain>"
        exit 2
      fi
      rm -f "$site_dir/$domain.conf" "$enabled_site_dir/$domain.conf"
      append_operation "site-remove:$service_name"
      echo "OK: service=$service_name site=$domain removed=1"
    '';

    "site-list" = mkCommand "site-list" ''
      append_operation "site-list:$service_name"
      found=0
      for site_path in "$site_dir"/*.conf; do
        if [ ! -e "$site_path" ]; then
          continue
        fi
        found=1
        site_name="$(basename "$site_path" .conf)"
        enabled_flag=0
        if [ -L "$enabled_site_dir/$site_name.conf" ]; then
          enabled_flag=1
        fi
        echo "INFO: site=$site_name enabled=$enabled_flag"
      done
      if [ "$found" -eq 0 ]; then
        echo "INFO: site=none"
      fi
    '';

    "site-enable" = mkCommand "site-enable" ''
      domain="''${1:-}"
      if [ -z "$domain" ]; then
        echo "ERROR: usage: site-enable <domain>"
        exit 2
      fi
      if [ ! -f "$site_dir/$domain.conf" ]; then
        echo "ERROR: site missing domain=$domain"
        exit 1
      fi
      ln -sf "../sites/$domain.conf" "$enabled_site_dir/$domain.conf"
      append_operation "site-enable:$service_name"
      echo "OK: service=$service_name site=$domain enabled=1"
    '';

    "site-disable" = mkCommand "site-disable" ''
      domain="''${1:-}"
      if [ -z "$domain" ]; then
        echo "ERROR: usage: site-disable <domain>"
        exit 2
      fi
      rm -f "$enabled_site_dir/$domain.conf"
      append_operation "site-disable:$service_name"
      echo "OK: service=$service_name site=$domain enabled=0"
    '';

    "cert-obtain" = mkCommand "cert-obtain" ''
      domain="''${1:-}"
      email="''${2:-}"
      if [ -z "$domain" ] || [ -z "$email" ]; then
        echo "ERROR: usage: cert-obtain <domain> <email>"
        exit 2
      fi
      printf 'domain=%s\nemail=%s\n' "$domain" "$email" > "$cert_dir/$domain.pem"
      append_operation "cert-obtain:$service_name"
      echo "OK: service=$service_name cert=$domain obtained=1"
    '';

    "cert-renew" = mkCommand "cert-renew" ''
      append_operation "cert-renew:$service_name"
      echo "OK: service=$service_name certs=renewed"
    '';

    "cert-status" = mkCommand "cert-status" ''
      append_operation "cert-status:$service_name"
      found=0
      for cert_path in "$cert_dir"/*.pem; do
        if [ ! -e "$cert_path" ]; then
          continue
        fi
        found=1
        echo "INFO: cert=$(basename "$cert_path" .pem) status=present"
      done
      if [ "$found" -eq 0 ]; then
        echo "INFO: cert=none status=absent"
      fi
    '';
  };
}
