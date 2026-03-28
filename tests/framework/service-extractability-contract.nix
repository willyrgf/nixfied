{ pkgs }:
let
  lib = pkgs.lib;
  frameworkLib = import ../../nixfied/framework/core {
    inherit
      pkgs
      ;
    system = pkgs.system;
  };

  enableAllServicesModule =
    { lib, ... }:
    {
      nixfied.services.postgres.enable = lib.mkForce true;
      nixfied.services.nginx.enable = lib.mkForce true;
      nixfied.services.minio.enable = lib.mkForce true;
      nixfied.services.reth.enable = lib.mkForce true;
      nixfied.services.helios.enable = lib.mkForce true;
    };

  compiled = frameworkLib.mkNixfied {
    projectRoot = ../..;
    projectModules = [ ../../nixfied/project/module.nix ];
    extraModules = [ enableAllServicesModule ];
    localOverrides = [ ];
  };

  catalog = compiled.model.compiled.serviceSurfaceCatalog;
  sortKeys = attrs: builtins.sort builtins.lessThan (builtins.attrNames attrs);
  serviceNames = sortKeys (catalog.serviceApis or { });

  compileSource = builtins.readFile ../../nixfied/compiler/compile-service-surface-catalog.nix;
  runtimeSurfaceSource = builtins.readFile ../../nixfied/framework/core/mkServiceRuntimeSurfaces.nix;

  moduleSources = {
    postgres = builtins.readFile ../../nixfied/modules/services/postgres.nix;
    nginx = builtins.readFile ../../nixfied/modules/services/nginx.nix;
    minio = builtins.readFile ../../nixfied/modules/services/minio.nix;
    reth = builtins.readFile ../../nixfied/modules/services/reth.nix;
    helios = builtins.readFile ../../nixfied/modules/services/helios.nix;
  };

  adapterSources = {
    postgres = builtins.readFile ../../nixfied/framework/runtime/services/postgres/default.nix;
    nginx = builtins.readFile ../../nixfied/framework/runtime/services/nginx/default.nix;
    minio = builtins.readFile ../../nixfied/framework/runtime/services/minio/default.nix;
    reth = builtins.readFile ../../nixfied/framework/runtime/services/reth/default.nix;
    helios = builtins.readFile ../../nixfied/framework/runtime/services/helios/default.nix;
  };

  contractOwnerMatches =
    serviceName:
    catalog.serviceApis.${serviceName}.ownerFile == "nixfied/modules/services/${serviceName}.nix";

  contractAdapterMatches =
    serviceName:
    builtins.match ".*framework/runtime/services/${serviceName}/default\\.nix" (
      toString catalog.serviceApis.${serviceName}.adapter.module
    ) != null;

  moduleDefinesTypedContract =
    serviceName:
    let
      source = moduleSources.${serviceName};
    in
    lib.hasInfix "contract = contractSchema.mkContractOption" source
    && lib.hasInfix "config.nixfied.services.${serviceName}.contract = {" source
    && lib.hasInfix "mkObservabilityOperations" source;

  adapterIsPrivateOnly =
    serviceName:
    let
      source = adapterSources.${serviceName};
    in
    lib.hasInfix "version = 1;" source
    && lib.hasInfix "operations = {" source
    && !(lib.hasInfix "publicApi" source)
    && !(lib.hasInfix "serviceModule" source);
in
assert
  serviceNames == [
    "helios"
    "minio"
    "nginx"
    "postgres"
    "reth"
  ];
assert serviceNames == sortKeys compiled.serviceApis;
assert lib.all contractOwnerMatches serviceNames;
assert lib.all contractAdapterMatches serviceNames;
assert lib.all moduleDefinesTypedContract serviceNames;
assert lib.all adapterIsPrivateOnly serviceNames;
assert !(lib.hasInfix "serviceModulePath" compileSource);
assert !(lib.hasInfix "serviceProject =" compileSource);
assert !(lib.hasInfix "slots =" compileSource);
assert !(lib.hasInfix "mkServiceApisFromModules" compileSource);
assert !(lib.hasInfix "publicApi" compileSource);
assert lib.hasInfix "serviceDefinitions" compileSource;
assert !(lib.hasInfix "mkServiceApisFromModules" runtimeSurfaceSource);
assert !(lib.hasInfix "publicApi" runtimeSurfaceSource);
assert lib.hasInfix "adapter.module" runtimeSurfaceSource;
assert builtins.hasAttr "svc::postgres::status" compiled.apps;
assert builtins.hasAttr "svc::nginx::site-add" compiled.apps;
assert builtins.hasAttr "svc::minio::bucket-ensure" compiled.apps;
assert builtins.hasAttr "svc::reth::ready" compiled.apps;
assert builtins.hasAttr "svc::helios::ready" compiled.apps;
assert !(builtins.hasAttr "svc::postgres::preflight-init" compiled.apps);
assert !(builtins.hasAttr "svc::postgres::ensure-migration-tested" compiled.apps);
assert !(builtins.hasAttr "svc::nginx::site-proxy" compiled.apps);
assert builtins.hasAttr "SVC_POSTGRES_STATUS" compiled.serviceHookEnv;
assert builtins.hasAttr "SVC_NGINX_SITE_ADD" compiled.serviceHookEnv;
assert builtins.hasAttr "SVC_MINIO_BUCKET_ENSURE" compiled.serviceHookEnv;
assert builtins.hasAttr "SVC_RETH_READY" compiled.serviceHookEnv;
assert builtins.hasAttr "SVC_HELIOS_READY" compiled.serviceHookEnv;
assert !(builtins.hasAttr "SVC_POSTGRES_PREFLIGHT_START" compiled.serviceHookEnv);
pkgs.runCommand "service-extractability-contract" { } ''
  echo "OK: real built-in services declare typed module contracts, the compiler reads those contracts without importing runtime implementations, and runtime surfaces project through adapter-only service modules" > "$out"
''
