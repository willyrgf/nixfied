{ pkgs }:
let
  helperSource = builtins.readFile ../../nixfied/.framework/lib/managed-service-lifecycle.nix;
  minioSource = builtins.readFile ../../nixfied/.framework/minio/lifecycle.nix;
  rethSource = builtins.readFile ../../nixfied/.framework/reth/lifecycle.nix;
  heliosSource = builtins.readFile ../../nixfied/.framework/helios/lifecycle.nix;
  nginxSource = builtins.readFile ../../nixfied/.framework/nginx/default.nix;
  postgresSource = builtins.readFile ../../nixfied/.framework/postgres/default.nix;
in
assert pkgs.lib.hasInfix "mkPidFileManagedLifecycle" helperSource;
assert pkgs.lib.hasInfix "mkWrappedScript" helperSource;
assert pkgs.lib.hasInfix "mkObservedStatusScript" helperSource;
assert pkgs.lib.hasInfix "SERVICE_PID_FILE" helperSource;
assert pkgs.lib.hasInfix "SERVICE_LOG_FILE" helperSource;
assert pkgs.lib.hasInfix "managed-service-lifecycle.nix" minioSource;
assert pkgs.lib.hasInfix "managed-service-lifecycle.nix" rethSource;
assert pkgs.lib.hasInfix "managed-service-lifecycle.nix" heliosSource;
assert pkgs.lib.hasInfix "managed-service-lifecycle.nix" nginxSource;
assert pkgs.lib.hasInfix "managed-service-lifecycle.nix" postgresSource;
assert pkgs.lib.hasInfix "SERVICE_PID_FILE=\"$MINIO_PID_FILE\"" minioSource;
assert pkgs.lib.hasInfix "SERVICE_PID_FILE=\"$RETH_PID_FILE\"" rethSource;
assert pkgs.lib.hasInfix "SERVICE_PID_FILE=\"$HELIOS_PID_FILE\"" heliosSource;
assert pkgs.lib.hasInfix "managedLifecycle = managedServiceLifecycle.mkPidFileManagedLifecycle" minioSource;
assert pkgs.lib.hasInfix "managedLifecycle = managedServiceLifecycle.mkPidFileManagedLifecycle" rethSource;
assert pkgs.lib.hasInfix "managedLifecycle = managedServiceLifecycle.mkPidFileManagedLifecycle" heliosSource;
assert pkgs.lib.hasInfix "inherit (managedLifecycle)" minioSource;
assert pkgs.lib.hasInfix "inherit (managedLifecycle)" rethSource;
assert pkgs.lib.hasInfix "inherit (managedLifecycle)" heliosSource;
assert pkgs.lib.hasInfix "mkObservedStatusScript" nginxSource;
assert pkgs.lib.hasInfix "mkWrappedScript" nginxSource;
assert pkgs.lib.hasInfix "mkObservedStatusScript" postgresSource;
assert pkgs.lib.hasInfix "mkWrappedScript" postgresSource;
pkgs.runCommand "managed-service-lifecycle-contract" { } ''
  echo "OK: managed service lifecycle helper is wired into minio/reth/helios/nginx/postgres" > "$out"
''
