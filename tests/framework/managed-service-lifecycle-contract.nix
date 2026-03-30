{ pkgs }:
let
  lib = pkgs.lib;
  plainShellLogging = import ../../nixfied/framework/core/plain-shell-logging.nix;
  helperSource = builtins.readFile ../../nixfied/framework/runtime/helpers/managed-service-lifecycle.nix;
  probeCommandsSource = builtins.readFile ../../nixfied/framework/core/probe-commands.nix;
  probePlanRuntimeSource = builtins.readFile ../../nixfied/framework/core/probe-plan-runtime.nix;
  minioSource = builtins.readFile ../../nixfied/framework/runtime/services/minio/lifecycle.nix;
  rethSource = builtins.readFile ../../nixfied/framework/runtime/services/reth/lifecycle.nix;
  heliosSource = builtins.readFile ../../nixfied/framework/runtime/services/helios/lifecycle.nix;
  nginxLifecycleSource = builtins.readFile ../../nixfied/framework/runtime/services/nginx/lifecycle.nix;
  nginxDefaultSource = builtins.readFile ../../nixfied/framework/runtime/services/nginx/default.nix;
  postgresLifecycleSource = builtins.readFile ../../nixfied/framework/runtime/services/postgres/lifecycle.nix;
  postgresDefaultSource = builtins.readFile ../../nixfied/framework/runtime/services/postgres/default.nix;

  loggingPrelude = plainShellLogging { };

  mkPackageWithScript =
    {
      name,
      binName,
      script,
      extraSetup ? "",
    }:
    pkgs.runCommand name { } ''
      mkdir -p "$out/bin"
      cp ${script} "$out/bin/${binName}"
      chmod +x "$out/bin/${binName}"
      ${extraSetup}
    '';

  nginxStubScript = pkgs.writeShellScript "managed-service-contract-nginx-stub" ''
    exit 0
  '';
  nginxStub = mkPackageWithScript {
    name = "managed-service-contract-nginx";
    binName = "nginx";
    script = nginxStubScript;
    extraSetup = ''
      mkdir -p "$out/conf"
      printf 'types {}\n' > "$out/conf/mime.types"
    '';
  };
  minioStub = pkgs.writeShellScriptBin "minio" ''
    exit 0
  '';
  rethStub = pkgs.writeShellScriptBin "reth" ''
    exit 0
  '';
  heliosStub = pkgs.writeShellScriptBin "helios" ''
    exit 0
  '';

  slotsStub =
    let
      slotInfo = pkgs.writeShellScript "managed-service-contract-slot-info" ''
        printf 'SLOT=%q\n' "''${SLOT:-0}"
        printf 'ENV=%q\n' "''${ENV:-test}"
        printf 'SERVICE_ROOT=%q\n' "''${SERVICE_ROOT:-/tmp/service-root}"
        printf 'HTTP_PORT=%q\n' "''${HTTP_PORT:-28080}"
        printf 'HTTPS_PORT=%q\n' "''${HTTPS_PORT:-28443}"
        printf 'MINIO_API_PORT=%q\n' "''${MINIO_API_PORT:-29000}"
        printf 'MINIO_CONSOLE_PORT=%q\n' "''${MINIO_CONSOLE_PORT:-29001}"
        printf 'RETH_HTTP_PORT=%q\n' "''${RETH_HTTP_PORT:-29100}"
        printf 'RETH_WS_PORT=%q\n' "''${RETH_WS_PORT:-29101}"
        printf 'RETH_AUTH_PORT=%q\n' "''${RETH_AUTH_PORT:-29102}"
        printf 'HELIOSRPC_PORT=%q\n' "''${HELIOSRPC_PORT:-29200}"
      '';
    in
    {
      getSlotInfo = slotInfo;
      getServiceDir = name: "\${SERVICE_ROOT}/${name}";
      portVarName =
        key:
        if key == "http" then
          "HTTP_PORT"
        else if key == "https" then
          "HTTPS_PORT"
        else if key == "minioApi" then
          "MINIO_API_PORT"
        else if key == "minioConsole" then
          "MINIO_CONSOLE_PORT"
        else if key == "rethHttp" then
          "RETH_HTTP_PORT"
        else if key == "rethWs" then
          "RETH_WS_PORT"
        else if key == "rethAuth" then
          "RETH_AUTH_PORT"
        else if key == "heliosRpc" then
          "HELIOSRPC_PORT"
        else
          throw "unsupported managed-service contract port key ${key}";
    };

  projectBase = {
    project = {
      id = "managed-service-lifecycle-contract";
    };
  };

  nginxProject = projectBase // {
    services.nginx = {
      defaultSource = "stub";
      sources.stub.package = nginxStub;
    };
  };
  minioProject = projectBase // {
    services.minio = {
      defaultSource = "stub";
      sources.stub.package = minioStub;
    };
  };
  rethProject = projectBase // {
    services.reth = {
      defaultSource = "stub";
      sources.stub.package = rethStub;
    };
  };
  heliosProject = projectBase // {
    services.helios = {
      defaultSource = "stub";
      sources.stub.package = heliosStub;
    };
  };

  nginxConfig = import ../../nixfied/framework/runtime/services/nginx/config.nix {
    inherit pkgs;
    project = nginxProject;
  };
  nginxTemplates = import ../../nixfied/framework/runtime/services/nginx/templates.nix {
    inherit pkgs;
    package = nginxConfig.package or pkgs.nginx;
  };
  nginxLifecycle = import ../../nixfied/framework/runtime/services/nginx/lifecycle.nix {
    inherit pkgs;
    project = nginxProject;
    slots = slotsStub;
    config = nginxConfig;
    templates = nginxTemplates;
    inherit loggingPrelude;
  };

  minioConfig = import ../../nixfied/framework/runtime/services/minio/config.nix {
    inherit pkgs;
    project = minioProject;
  };
  minioLifecycle = import ../../nixfied/framework/runtime/services/minio/lifecycle.nix {
    inherit pkgs;
    project = minioProject;
    slots = slotsStub;
    config = minioConfig;
    inherit loggingPrelude;
  };

  rethConfig = import ../../nixfied/framework/runtime/services/reth/config.nix {
    inherit pkgs;
    project = rethProject;
  };
  rethLifecycle = import ../../nixfied/framework/runtime/services/reth/lifecycle.nix {
    inherit pkgs;
    project = rethProject;
    slots = slotsStub;
    config = rethConfig;
    inherit loggingPrelude;
  };

  heliosConfig = import ../../nixfied/framework/runtime/services/helios/config.nix {
    inherit pkgs;
    project = heliosProject;
  };
  heliosLifecycle = import ../../nixfied/framework/runtime/services/helios/lifecycle.nix {
    inherit pkgs;
    project = heliosProject;
    slots = slotsStub;
    config = heliosConfig;
    inherit loggingPrelude;
  };

  shellVarRef = name: "$" + "{" + name + "}";
  generatedServiceScripts = [
    {
      name = "nginx";
      dirVar = "NGINX_DIR";
      pidVar = "NGINX_PID_FILE";
      logVar = "NGINX_LOG_FILE";
      pidFileName = "nginx.pid";
      logFileName = "error.log";
      status = builtins.readFile nginxLifecycle.status;
      stop = builtins.readFile nginxLifecycle.stop;
    }
    {
      name = "minio";
      dirVar = "MINIO_DIR";
      pidVar = "MINIO_PID_FILE";
      logVar = "MINIO_LOG_FILE";
      pidFileName = "minio.pid";
      logFileName = "minio.log";
      status = builtins.readFile minioLifecycle.status;
      stop = builtins.readFile minioLifecycle.stop;
    }
    {
      name = "reth";
      dirVar = "RETH_DIR";
      pidVar = "RETH_PID_FILE";
      logVar = "RETH_LOG_FILE";
      pidFileName = "reth.pid";
      logFileName = "reth.log";
      status = builtins.readFile rethLifecycle.status;
      stop = builtins.readFile rethLifecycle.stop;
    }
    {
      name = "helios";
      dirVar = "HELIOS_DIR";
      pidVar = "HELIOS_PID_FILE";
      logVar = "HELIOS_LOG_FILE";
      pidFileName = "helios.pid";
      logFileName = "helios.log";
      status = builtins.readFile heliosLifecycle.status;
      stop = builtins.readFile heliosLifecycle.stop;
    }
  ];

  assertGeneratedPreludeContract =
    service:
    let
      expectedPidLine =
        service.pidVar + "=\"" + shellVarRef service.dirVar + "/run/" + service.pidFileName + "\"";
      expectedLogLine =
        service.logVar + "=\"" + shellVarRef service.dirVar + "/logs/" + service.logFileName + "\"";
      expectedServiceDirLine = "SERVICE_DIR=\"" + shellVarRef service.dirVar + "\"";
      expectedServicePidLine = "SERVICE_PID_FILE=\"" + shellVarRef service.pidVar + "\"";
      expectedServiceLogLine = "SERVICE_LOG_FILE=\"" + shellVarRef service.logVar + "\"";
      invalidFragments = [
        "$${svcDir}"
        "$${svcPidFile}"
        "$${svcLogFile}"
      ];
      scriptChecks =
        script:
        assert lib.hasInfix expectedPidLine script;
        assert lib.hasInfix expectedLogLine script;
        assert lib.hasInfix expectedServiceDirLine script;
        assert lib.hasInfix expectedServicePidLine script;
        assert lib.hasInfix expectedServiceLogLine script;
        assert builtins.all (fragment: !(lib.hasInfix fragment script)) invalidFragments;
        true;
    in
    assert scriptChecks service.status;
    assert scriptChecks service.stop;
    true;

  generatedPreludeChecks = map assertGeneratedPreludeContract generatedServiceScripts;
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
assert pkgs.lib.hasInfix "startPostLaunchBody = managedServiceLifecycle.mkStartupReadinessBody {"
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
assert builtins.all (x: x) generatedPreludeChecks;
pkgs.runCommand "managed-service-lifecycle-contract" { } ''
  echo "OK: managed service lifecycle helper owns shared nginx/postgres/minio/reth/helios lifecycle scaffolding" > "$out"
''
