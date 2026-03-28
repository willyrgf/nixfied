{
  pkgs,
  serviceCatalog,
  apps,
}:
let
  postgresSource = builtins.readFile ../../nixfied/framework/runtime/services/postgres/default.nix;
  nginxSource = builtins.readFile ../../nixfied/framework/runtime/services/nginx/default.nix;
  minioSource = builtins.readFile ../../nixfied/framework/runtime/services/minio/default.nix;
  minioBucketMgmtSource = builtins.readFile ../../nixfied/framework/runtime/services/minio/bucket-management.nix;
  rethSource = builtins.readFile ../../nixfied/framework/runtime/services/reth/default.nix;
  heliosSource = builtins.readFile ../../nixfied/framework/runtime/services/helios/default.nix;
  supervisorSource = builtins.readFile ../../nixfied/framework/runtime/services/supervisor/default.nix;

  assertAdapterOnlySource =
    source:
    assert pkgs.lib.hasInfix "version = 1;" source;
    assert pkgs.lib.hasInfix "operations = {" source;
    assert (!pkgs.lib.hasInfix "publicApi" source);
    assert (!pkgs.lib.hasInfix "serviceModule" source);
    true;

  serviceStatusAppMatchesEnable =
    serviceName:
    let
      appName = "svc::${serviceName}::status";
      expectedEnabled = builtins.any (
        serviceId:
        let
          service = serviceCatalog.${serviceId};
        in
        (service.name or serviceId) == serviceName && (service.enable or false)
      ) (builtins.attrNames serviceCatalog);
    in
    assert (builtins.hasAttr appName apps) == expectedEnabled;
    true;
in
assert assertAdapterOnlySource postgresSource;
assert assertAdapterOnlySource nginxSource;
assert assertAdapterOnlySource minioSource;
assert assertAdapterOnlySource rethSource;
assert assertAdapterOnlySource heliosSource;
assert pkgs.lib.hasInfix "bucketMgmt = import ./bucket-management.nix" minioSource;
assert pkgs.lib.hasInfix "runtimeDefaults = import ../../../core/runtime-defaults.nix;"
  minioBucketMgmtSource;
assert pkgs.lib.hasInfix "mkBucketScript =" minioBucketMgmtSource;
assert pkgs.lib.hasInfix "mcAliasSetup =" minioBucketMgmtSource;
assert serviceStatusAppMatchesEnable "postgres";
assert serviceStatusAppMatchesEnable "nginx";
assert serviceStatusAppMatchesEnable "minio";
assert serviceStatusAppMatchesEnable "reth";
assert serviceStatusAppMatchesEnable "helios";
assert !(builtins.hasAttr "svc::postgres::preflight-init" apps);
assert !(builtins.hasAttr "svc::postgres::preflight-start" apps);
assert !(builtins.hasAttr "svc::nginx::preflight-start" apps);
assert !(builtins.hasAttr "svc::minio::preflight-start" apps);
assert !(builtins.hasAttr "svc::reth::preflight-start" apps);
assert !(builtins.hasAttr "svc::helios::preflight-start" apps);
assert pkgs.lib.hasInfix "inherit (lifecycle)" supervisorSource;
assert pkgs.lib.hasInfix "start" supervisorSource;
assert pkgs.lib.hasInfix "stop" supervisorSource;
assert pkgs.lib.hasInfix "startDaemon" supervisorSource;
assert pkgs.lib.hasInfix "inherit (statusMod)" supervisorSource;
assert pkgs.lib.hasInfix "status" supervisorSource;
assert pkgs.lib.hasInfix "health" supervisorSource;
assert pkgs.lib.hasInfix "inherit (management) restart rotateLogs;" supervisorSource;
assert (!pkgs.lib.hasInfix "ready" supervisorSource);
pkgs.runCommand "service-api-surface-contract" { } ''
  echo "OK: runtime service modules expose adapter-only operation maps, public service apps still track enabled services, and supervisor remains a separate runtime surface" > "$out"
''
