{ pkgs }:
let
  helperSource = builtins.readFile ../../nixfied/.framework/lib/managed-service-lifecycle.nix;
  minioSource = builtins.readFile ../../nixfied/.framework/minio/lifecycle.nix;
  rethSource = builtins.readFile ../../nixfied/.framework/reth/lifecycle.nix;
  heliosSource = builtins.readFile ../../nixfied/.framework/helios/lifecycle.nix;
  nginxLifecycleSource = builtins.readFile ../../nixfied/.framework/nginx/lifecycle.nix;
  nginxDefaultSource = builtins.readFile ../../nixfied/.framework/nginx/default.nix;
  postgresLifecycleSource = builtins.readFile ../../nixfied/.framework/postgres/lifecycle.nix;
  postgresDefaultSource = builtins.readFile ../../nixfied/.framework/postgres/default.nix;
in
assert pkgs.lib.hasInfix "mkPidFileManagedLifecycle" helperSource;
assert pkgs.lib.hasInfix "mkWrappedScript" helperSource;
assert pkgs.lib.hasInfix "mkObservedStatusScript" helperSource;
assert pkgs.lib.hasInfix "mkStopOutcomeBody" helperSource;
assert pkgs.lib.hasInfix "mkProcessExitFailureBody" helperSource;
assert pkgs.lib.hasInfix "mkSimpleProbeBody" helperSource;
assert pkgs.lib.hasInfix "mkStartupReadinessBody" helperSource;
assert pkgs.lib.hasInfix "print_log_tail \"$LOG_FILE\"" helperSource;
assert pkgs.lib.hasInfix "SERVICE_PID_FILE" helperSource;
assert pkgs.lib.hasInfix "SERVICE_LOG_FILE" helperSource;
assert pkgs.lib.hasInfix "stopRequestBody" helperSource;
assert pkgs.lib.hasInfix "stopMissingLogPathExpr" helperSource;
assert pkgs.lib.hasInfix "stopStateLogPathExpr" helperSource;
assert pkgs.lib.hasInfix "managed-service-lifecycle.nix" minioSource;
assert pkgs.lib.hasInfix "managed-service-lifecycle.nix" rethSource;
assert pkgs.lib.hasInfix "managed-service-lifecycle.nix" heliosSource;
assert pkgs.lib.hasInfix "managed-service-lifecycle.nix" nginxLifecycleSource;
assert pkgs.lib.hasInfix "managed-service-lifecycle.nix" postgresLifecycleSource;
assert pkgs.lib.hasInfix "SERVICE_PID_FILE=\"$MINIO_PID_FILE\"" minioSource;
assert pkgs.lib.hasInfix "SERVICE_PID_FILE=\"$RETH_PID_FILE\"" rethSource;
assert pkgs.lib.hasInfix "SERVICE_PID_FILE=\"$HELIOS_PID_FILE\"" heliosSource;
assert pkgs.lib.hasInfix "managedLifecycle = managedServiceLifecycle.mkPidFileManagedLifecycle"
  minioSource;
assert pkgs.lib.hasInfix "managedLifecycle = managedServiceLifecycle.mkPidFileManagedLifecycle"
  rethSource;
assert pkgs.lib.hasInfix "managedLifecycle = managedServiceLifecycle.mkPidFileManagedLifecycle"
  heliosSource;
assert pkgs.lib.hasInfix "managedLifecycle = serviceScripts.mkPidFileManagedLifecycle"
  nginxLifecycleSource;
assert pkgs.lib.hasInfix "stopMissingLogPathExpr = null;" minioSource;
assert (!pkgs.lib.hasInfix "stopMissingBody =" minioSource);
assert (!pkgs.lib.hasInfix "stopStaleBody =" minioSource);
assert (!pkgs.lib.hasInfix "stopStoppedBody =" minioSource);
assert (!pkgs.lib.hasInfix "stopForceKilledBody =" minioSource);
assert (!pkgs.lib.hasInfix "stopMissingBody =" rethSource);
assert (!pkgs.lib.hasInfix "stopStaleBody =" rethSource);
assert (!pkgs.lib.hasInfix "stopStoppedBody =" rethSource);
assert (!pkgs.lib.hasInfix "stopForceKilledBody =" rethSource);
assert (!pkgs.lib.hasInfix "stopMissingBody =" heliosSource);
assert (!pkgs.lib.hasInfix "stopStaleBody =" heliosSource);
assert (!pkgs.lib.hasInfix "stopStoppedBody =" heliosSource);
assert (!pkgs.lib.hasInfix "stopForceKilledBody =" heliosSource);
assert (!pkgs.lib.hasInfix "stopMissingBody =" nginxLifecycleSource);
assert (!pkgs.lib.hasInfix "stopStaleBody =" nginxLifecycleSource);
assert (!pkgs.lib.hasInfix "stopStoppedBody =" nginxLifecycleSource);
assert (!pkgs.lib.hasInfix "stopForceKilledBody =" nginxLifecycleSource);
assert pkgs.lib.hasInfix "startExitFailureBody = managedServiceLifecycle.mkProcessExitFailureBody {"
  minioSource;
assert pkgs.lib.hasInfix "startPostLaunchBody = serviceScripts.mkStartupReadinessBody {"
  nginxLifecycleSource;
assert pkgs.lib.hasInfix "startExitFailureBody = serviceScripts.mkProcessExitFailureBody {"
  nginxLifecycleSource;
assert pkgs.lib.hasInfix "healthBody = managedServiceLifecycle.mkSimpleProbeBody" minioSource;
assert pkgs.lib.hasInfix "readyBody = managedServiceLifecycle.mkSimpleProbeBody" minioSource;
assert pkgs.lib.hasInfix "healthBody = serviceScripts.mkSimpleProbeBody" nginxLifecycleSource;
assert pkgs.lib.hasInfix "readyBody = serviceScripts.mkSimpleProbeBody" nginxLifecycleSource;
assert pkgs.lib.hasInfix "healthBody = managedServiceLifecycle.mkSimpleProbeBody" rethSource;
assert pkgs.lib.hasInfix "readyBody = managedServiceLifecycle.mkSimpleProbeBody" rethSource;
assert pkgs.lib.hasInfix "startPostLaunchBody = managedServiceLifecycle.mkStartupReadinessBody {"
  rethSource;
assert pkgs.lib.hasInfix "startExitFailureBody = managedServiceLifecycle.mkProcessExitFailureBody {"
  rethSource;
assert pkgs.lib.hasInfix "healthBody = managedServiceLifecycle.mkSimpleProbeBody" heliosSource;
assert pkgs.lib.hasInfix "startPostLaunchBody = managedServiceLifecycle.mkStartupReadinessBody {"
  heliosSource;
assert pkgs.lib.hasInfix "startExitFailureBody = managedServiceLifecycle.mkProcessExitFailureBody {"
  heliosSource;
assert pkgs.lib.hasInfix "body = managedServiceLifecycle.mkSimpleProbeBody" postgresLifecycleSource;
assert pkgs.lib.hasInfix "print_log_tail \"$PGDATA/postgres.log\" 20 \"postgres\""
  postgresLifecycleSource;
assert pkgs.lib.hasInfix "inherit (managedLifecycle)" minioSource;
assert pkgs.lib.hasInfix "inherit (managedLifecycle)" rethSource;
assert pkgs.lib.hasInfix "inherit (managedLifecycle)" heliosSource;
assert pkgs.lib.hasInfix "inherit (managedLifecycle)" nginxLifecycleSource;
assert pkgs.lib.hasInfix "mkObservedStatusScript" postgresLifecycleSource;
assert pkgs.lib.hasInfix "mkPgScript" postgresLifecycleSource;
assert (!pkgs.lib.hasInfix "mkObservedStatusScript" nginxDefaultSource);
assert (!pkgs.lib.hasInfix "mkWrappedScript" nginxDefaultSource);
assert (!pkgs.lib.hasInfix "mkObservedStatusScript" postgresDefaultSource);
assert (!pkgs.lib.hasInfix "mkWrappedScript" postgresDefaultSource);
pkgs.runCommand "managed-service-lifecycle-contract" { } ''
  echo "OK: managed service lifecycle helper owns shared nginx/postgres/minio/reth/helios lifecycle scaffolding" > "$out"
''
