{
  lib,
  config,
  ...
}:
let
  t = lib.types;
  cfg = config.nixfied.services.nginx;
  probeLib = import ./probes.nix { inherit lib; };
  contractSchema = import ./contract-schema.nix { inherit lib; };
  operationContractBuilder = import ./operation-contract-builder.nix;
  sourceOptions = import ./source-options.nix { inherit lib; };
  sourceSpec = sourceOptions.mkSourceSpec { };
  serviceDir = contractSchema.mkServiceDirExpr cfg.dataDirName;
in
{
  options.nixfied.services.nginx = {
    enable = lib.mkOption {
      type = t.bool;
      default = false;
    };
    portKeyHttp = lib.mkOption {
      type = t.str;
      default = "http";
    };
    portKeyHttps = lib.mkOption {
      type = t.str;
      default = "https";
    };
    dataDirName = lib.mkOption {
      type = t.str;
      default = "nginx";
    };
    sources = lib.mkOption {
      type = t.attrsOf sourceSpec;
      default = { };
    };
    sourceKeys = lib.mkOption {
      type = t.listOf t.str;
      default = [ ];
    };
    defaultSource = lib.mkOption {
      type = t.str;
      default = "";
    };
    probes = probeLib.probeOptions;
    contract = contractSchema.mkContractOption "Typed nginx public contract.";
    implementation = contractSchema.mkImplementationOption "Private nginx runtime implementation.";
  };

  config.nixfied.services.nginx.contract = {
    version = 1;
    service = "nginx";
    summary = "Nginx service management API";
    details = "Public service contract for managing nginx across dev/prod/test/ci.";
    ownerFile = "nixfied/modules/services/nginx.nix";
    artifacts = {
      httpPortVar = contractSchema.mkPortVarName cfg.portKeyHttp;
      httpsPortVar = contractSchema.mkPortVarName cfg.portKeyHttps;
      serviceDir = serviceDir;
      dataDir = serviceDir;
      logFile = "${serviceDir}/logs/error.log";
      pidFile = "${serviceDir}/run/nginx.pid";
    };
    runtimePrimitives = contractSchema.mkRuntimePrimitivesV1 config.nixfied.runtime;
    operations =
      (operationContractBuilder {
        displayName = "nginx";
        extraOperations = {
          init = {
            runtimeOp = "init";
            summary = "Initialize nginx directories and config";
            details = "Creates nginx runtime directories and base configuration.";
          };

          start = {
            runtimeOp = "start-leaf";
            preOps = [
              "init"
              "check-config"
              "preflight-start"
            ];
            summary = "Start nginx server";
            details = "Starts nginx for the current slot and environment.";
          };

          reload = {
            runtimeOp = "reload";
            summary = "Reload nginx configuration";
            details = "Tests and reloads nginx configuration.";
          };

          list-instances = {
            runtimeOp = "list-instances";
            hook = "LIST_INSTANCES";
            summary = "List nginx instances";
            details = "Lists nginx instances managed by Nixfied.";
          };

          site-proxy = {
            runtimeOp = "site-proxy";
            hook = "SITE_PROXY";
            summary = "Write proxy site configuration";
            details = "Writes a proxy site config and enables it.";
            exposeApp = false;
            usage = [ "nix run .#svc::nginx::site-proxy -- <domain> <upstream-host> <upstream-port>" ];
          };

          site-static = {
            runtimeOp = "site-static";
            hook = "SITE_STATIC";
            summary = "Write static site configuration";
            details = "Writes a static site config and enables it.";
            exposeApp = false;
            usage = [ "nix run .#svc::nginx::site-static -- <domain> <site-root>" ];
          };

          site-add = {
            runtimeOp = "site-add";
            hook = "SITE_ADD";
            summary = "Add proxy nginx site";
            details = "Adds a proxy site and enables it.";
            usage = [ "nix run .#svc::nginx::site-add -- <domain> <upstream-host> <upstream-port>" ];
          };

          site-remove = {
            runtimeOp = "site-remove";
            hook = "SITE_REMOVE";
            summary = "Remove nginx site";
            details = "Removes nginx site configuration.";
            usage = [ "nix run .#svc::nginx::site-remove -- <domain>" ];
          };

          site-list = {
            runtimeOp = "site-list";
            hook = "SITE_LIST";
            summary = "List nginx sites";
            details = "Lists configured nginx sites.";
          };

          site-enable = {
            runtimeOp = "site-enable";
            hook = "SITE_ENABLE";
            summary = "Enable nginx site";
            details = "Enables an existing nginx site.";
            usage = [ "nix run .#svc::nginx::site-enable -- <domain>" ];
          };

          site-disable = {
            runtimeOp = "site-disable";
            hook = "SITE_DISABLE";
            summary = "Disable nginx site";
            details = "Disables an existing nginx site.";
            usage = [ "nix run .#svc::nginx::site-disable -- <domain>" ];
          };

          cert-obtain = {
            runtimeOp = "cert-obtain";
            hook = "CERT_OBTAIN";
            summary = "Obtain SSL certificate";
            details = "Obtains a Let's Encrypt certificate for a domain.";
            usage = [ "nix run .#svc::nginx::cert-obtain -- <domain> <email> [--staging]" ];
          };

          cert-renew = {
            runtimeOp = "cert-renew";
            hook = "CERT_RENEW";
            summary = "Renew SSL certificates";
            details = "Renews certificates for configured sites.";
          };

          cert-status = {
            runtimeOp = "cert-status";
            hook = "CERT_STATUS";
            summary = "Show SSL certificate status";
            details = "Prints certificate status for configured domains.";
          };
        };
      })
      // contractSchema.mkObservabilityOperations {
        service = "nginx";
        summaryName = "nginx";
      };
  };

  config.nixfied.services.nginx.implementation = {
    version = 1;
    module = ./runtime/nginx/default.nix;
  };
}
