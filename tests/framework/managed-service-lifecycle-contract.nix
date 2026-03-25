{ pkgs }:
let
  helperSource = builtins.readFile ../../nixfied/framework/runtime/helpers/managed-service-lifecycle.nix;
  probeCommandsSource = builtins.readFile ../../nixfied/framework/runtime/helpers/probe-commands.nix;
  probePlanRuntimeSource = builtins.readFile ../../nixfied/framework/runtime/helpers/probe-plan-runtime.nix;
  minioSource = builtins.readFile ../../nixfied/framework/runtime/services/minio/lifecycle.nix;
  rethSource = builtins.readFile ../../nixfied/framework/runtime/services/reth/lifecycle.nix;
  heliosSource = builtins.readFile ../../nixfied/framework/runtime/services/helios/lifecycle.nix;
  nginxLifecycleSource = builtins.readFile ../../nixfied/framework/runtime/services/nginx/lifecycle.nix;
  nginxDefaultSource = builtins.readFile ../../nixfied/framework/runtime/services/nginx/default.nix;
  postgresLifecycleSource = builtins.readFile ../../nixfied/framework/runtime/services/postgres/lifecycle.nix;
  postgresDefaultSource = builtins.readFile ../../nixfied/framework/runtime/services/postgres/default.nix;
in
assert pkgs.lib.hasInfix "mkPidFileManagedLifecycle" helperSource;
assert pkgs.lib.hasInfix "mkWrappedScript" helperSource;
assert pkgs.lib.hasInfix "mkObservedStatusScript" helperSource;
assert pkgs.lib.hasInfix "mkStopOutcomeBody" helperSource;
assert pkgs.lib.hasInfix "mkProcessExitFailureBody" helperSource;
assert pkgs.lib.hasInfix "mkReadyOutcomeBody" helperSource;
assert pkgs.lib.hasInfix "mkSimpleProbeBody" helperSource;
assert pkgs.lib.hasInfix "mkPlanProbeBody" helperSource;
assert pkgs.lib.hasInfix "mkStartupReadinessBody" helperSource;
assert pkgs.lib.hasInfix "nixfied-kernel probe jsonrpc" probeCommandsSource;
assert pkgs.lib.hasInfix "nixfied-kernel probe evaluate" probePlanRuntimeSource;
assert pkgs.lib.hasInfix ''kind = "nixfied-probe-execution-plan";'' probePlanRuntimeSource;
assert pkgs.lib.hasInfix "jsonRpcResultCompactCmd =" probeCommandsSource;
assert (!pkgs.lib.hasInfix "tmp_json=" probeCommandsSource);
assert (!pkgs.lib.hasInfix "export_file=" probeCommandsSource);
assert (!pkgs.lib.hasInfix "jsonRpcHasResultCmd" probePlanRuntimeSource);
assert (!pkgs.lib.hasInfix "jsonRpcResultHexCmd" probePlanRuntimeSource);
assert (!pkgs.lib.hasInfix "jsonRpcResultCompactCmd" probePlanRuntimeSource);
assert (!pkgs.lib.hasInfix "jsonRpcResultFalseCmd" probePlanRuntimeSource);
assert (!pkgs.lib.hasInfix "pgIsReadyCmd" probePlanRuntimeSource);
assert (!pkgs.lib.hasInfix "psqlQueryCmd" probePlanRuntimeSource);
assert pkgs.lib.hasInfix "import ../../core/runtime-defaults.nix" helperSource;
assert pkgs.lib.hasInfix "print_log_tail \"$LOG_FILE\"" helperSource;
assert pkgs.lib.hasInfix "SERVICE_PID_FILE" helperSource;
assert pkgs.lib.hasInfix "SERVICE_LOG_FILE" helperSource;
assert pkgs.lib.hasInfix "startPreflightBody ? \"\"" helperSource;
assert pkgs.lib.hasInfix "startPrepareBody ? \"\"" helperSource;
assert pkgs.lib.hasInfix "NIXFIED_START_RETURN_AFTER_READY" helperSource;
assert pkgs.lib.hasInfix "if [ \"''\${NIXFIED_START_RETURN_AFTER_READY:-0}\" = \"1\" ]; then"
  helperSource;
assert pkgs.lib.hasInfix "preflightStart = mkWrappedScript" helperSource;
assert pkgs.lib.hasInfix "startLeaf = mkWrappedScript" helperSource;
assert pkgs.lib.hasInfix "NIXFIED_START_RETURN_AFTER_READY=1 exec \${startLeaf}" helperSource;
assert pkgs.lib.hasInfix "fullStartLeaf = pkgs.writeShellScript" helperSource;
assert pkgs.lib.hasInfix "fullStartTestLeaf = pkgs.writeShellScript" helperSource;
assert pkgs.lib.hasInfix "stopRequestBody" helperSource;
assert pkgs.lib.hasInfix "stopMissingLogPathExpr" helperSource;
assert pkgs.lib.hasInfix "stopStateLogPathExpr" helperSource;
assert pkgs.lib.hasInfix "probe-setup-helper.nix" minioSource;
assert pkgs.lib.hasInfix "probe-setup-helper.nix" rethSource;
assert pkgs.lib.hasInfix "probe-setup-helper.nix" heliosSource;
assert pkgs.lib.hasInfix "probe-setup-helper.nix" nginxLifecycleSource;
assert pkgs.lib.hasInfix "probe-setup-helper.nix" postgresLifecycleSource;
assert pkgs.lib.hasInfix "probe-setup-helper.nix" minioSource;
assert pkgs.lib.hasInfix "probe-setup-helper.nix" rethSource;
assert pkgs.lib.hasInfix "probe-setup-helper.nix" heliosSource;
assert pkgs.lib.hasInfix "probe-setup-helper.nix" nginxLifecycleSource;
assert pkgs.lib.hasInfix "probe-setup-helper.nix" postgresLifecycleSource;
assert (!pkgs.lib.hasInfix "probeCommands.tcpOpenCmd" nginxLifecycleSource);
assert (!pkgs.lib.hasInfix "probeCommands.jsonRpcHasResultCmd" rethSource);
assert (!pkgs.lib.hasInfix "probeCommands.jsonRpcHasResultCmd" heliosSource);
assert (!pkgs.lib.hasInfix "probeCommands.pgIsReadyCmd" postgresLifecycleSource);
assert (!pkgs.lib.hasInfix "probeCommands.psqlQueryCmd" postgresLifecycleSource);
assert pkgs.lib.hasInfix "startupProbeCommand =" nginxLifecycleSource;
assert pkgs.lib.hasInfix "startupHealthCheck =" rethSource;
assert pkgs.lib.hasInfix "startupHealthCheck =" heliosSource;
assert pkgs.lib.hasInfix "renderQuietProbeStep =" postgresLifecycleSource;
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
assert pkgs.lib.hasInfix "startAlreadyRunningBody = managedServiceLifecycle.mkReadyOutcomeBody {"
  minioSource;
assert pkgs.lib.hasInfix "startPostLaunchBody = managedServiceLifecycle.mkReadyOutcomeBody {"
  minioSource;
assert pkgs.lib.hasInfix "startExitFailureBody = managedServiceLifecycle.mkProcessExitFailureBody {"
  minioSource;
assert pkgs.lib.hasInfix "startPostLaunchBody = serviceScripts.mkStartupReadinessBody {"
  nginxLifecycleSource;
assert pkgs.lib.hasInfix "startAlreadyRunningBody = serviceScripts.mkReadyOutcomeBody {"
  nginxLifecycleSource;
assert pkgs.lib.hasInfix "startExitFailureBody = serviceScripts.mkProcessExitFailureBody {"
  nginxLifecycleSource;
assert pkgs.lib.hasInfix "healthBody = managedServiceLifecycle.mkPlanProbeBody" minioSource;
assert pkgs.lib.hasInfix "readyBody = managedServiceLifecycle.mkPlanProbeBody" minioSource;
assert pkgs.lib.hasInfix "healthBody = managedServiceLifecycle.mkPlanProbeBody" minioSource;
assert pkgs.lib.hasInfix "readyBody = managedServiceLifecycle.mkPlanProbeBody" minioSource;
assert pkgs.lib.hasInfix "healthBody = serviceScripts.mkPlanProbeBody" nginxLifecycleSource;
assert pkgs.lib.hasInfix "readyBody = serviceScripts.mkPlanProbeBody" nginxLifecycleSource;
assert pkgs.lib.hasInfix "healthBody = serviceScripts.mkPlanProbeBody" nginxLifecycleSource;
assert pkgs.lib.hasInfix "readyBody = serviceScripts.mkPlanProbeBody" nginxLifecycleSource;
assert pkgs.lib.hasInfix "healthBody = managedServiceLifecycle.mkPlanProbeBody" rethSource;
assert pkgs.lib.hasInfix "readyBody = managedServiceLifecycle.mkPlanProbeBody" rethSource;
assert pkgs.lib.hasInfix "healthBody = managedServiceLifecycle.mkPlanProbeBody" rethSource;
assert pkgs.lib.hasInfix "readyBody = managedServiceLifecycle.mkPlanProbeBody" rethSource;
assert pkgs.lib.hasInfix "startPostLaunchBody = managedServiceLifecycle.mkStartupReadinessBody {"
  rethSource;
assert pkgs.lib.hasInfix "startAlreadyRunningBody = managedServiceLifecycle.mkReadyOutcomeBody {"
  rethSource;
assert pkgs.lib.hasInfix "startExitFailureBody = managedServiceLifecycle.mkProcessExitFailureBody {"
  rethSource;
assert pkgs.lib.hasInfix "healthBody = managedServiceLifecycle.mkPlanProbeBody" heliosSource;
assert pkgs.lib.hasInfix "readyBody =" heliosSource;
assert pkgs.lib.hasInfix "startPostLaunchBody = managedServiceLifecycle.mkStartupReadinessBody {"
  heliosSource;
assert pkgs.lib.hasInfix "startAlreadyRunningBody = managedServiceLifecycle.mkReadyOutcomeBody {"
  heliosSource;
assert pkgs.lib.hasInfix "startExitFailureBody = managedServiceLifecycle.mkProcessExitFailureBody {"
  heliosSource;
assert pkgs.lib.hasInfix "body = managedServiceLifecycle.mkPlanProbeBody" postgresLifecycleSource;
assert pkgs.lib.hasInfix "print_log_tail \"$PGDATA/postgres.log\" 20 \"postgres\""
  postgresLifecycleSource;
assert pkgs.lib.hasInfix "inherit (managedLifecycle)" minioSource;
assert pkgs.lib.hasInfix "inherit (managedLifecycle)" rethSource;
assert pkgs.lib.hasInfix "inherit (managedLifecycle)" heliosSource;
assert pkgs.lib.hasInfix "inherit (managedLifecycle)" nginxLifecycleSource;
assert pkgs.lib.hasInfix "mkObservedStatusScript" postgresLifecycleSource;
assert pkgs.lib.hasInfix "mkPgScript" postgresLifecycleSource;
assert (!pkgs.lib.hasInfix "healthBody = managedServiceLifecycle.mkSimpleProbeBody" minioSource);
assert (!pkgs.lib.hasInfix "readyBody = managedServiceLifecycle.mkSimpleProbeBody" minioSource);
assert (!pkgs.lib.hasInfix "healthBody = serviceScripts.mkSimpleProbeBody" nginxLifecycleSource);
assert (!pkgs.lib.hasInfix "readyBody = serviceScripts.mkSimpleProbeBody" nginxLifecycleSource);
assert (!pkgs.lib.hasInfix "healthBody = managedServiceLifecycle.mkSimpleProbeBody" rethSource);
assert (!pkgs.lib.hasInfix "readyBody = managedServiceLifecycle.mkSimpleProbeBody" rethSource);
assert (!pkgs.lib.hasInfix "healthBody = managedServiceLifecycle.mkSimpleProbeBody" heliosSource);
assert (
  !pkgs.lib.hasInfix "body = managedServiceLifecycle.mkSimpleProbeBody" postgresLifecycleSource
);
assert (!pkgs.lib.hasInfix "mkObservedStatusScript" nginxDefaultSource);
assert (!pkgs.lib.hasInfix "mkWrappedScript" nginxDefaultSource);
assert (!pkgs.lib.hasInfix "mkObservedStatusScript" postgresDefaultSource);
assert (!pkgs.lib.hasInfix "mkWrappedScript" postgresDefaultSource);
pkgs.runCommand "managed-service-lifecycle-contract" { } ''
  echo "OK: managed service lifecycle helper owns shared nginx/postgres/minio/reth/helios lifecycle scaffolding" > "$out"
''
