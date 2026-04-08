{ pkgs ? null }:
let
  project = {
    name = "Proof Workspace";
    id = "proof-workspace";
    description = "Canonical proof workspace fixture";
    envVar = "PROJECT_ENV";
    slotVar = "NIX_ENV";
  };
in
{
  inherit project;

  envs = {
    prod = {
      offset = 0;
    };
    dev = {
      offset = 10;
    };
    test = {
      offset = 20;
    };
  };

  slots = {
    max = 9;
    stride = 1;
    default = 0;
  };

  ports = {
    backend = 3000;
    frontend = 3100;
    http = 8080;
    https = 8443;
    postgres = 5432;
    minioApi = 9000;
    minioConsole = 9001;
    rethHttp = 8545;
    rethWs = 8546;
    rethAuth = 8551;
    heliosRpc = 8547;
    heliosExec = 8548;
  };

  logging = {
    level = "info";
    output = "stdout";
  };

  tooling = {
    runtimePackages =
      if pkgs == null then
        [ ]
      else
        [
          pkgs.coreutils
          pkgs.findutils
          pkgs.gnugrep
          pkgs.gnused
        ];
    devShellPackages = [ ];
    devShellHook = ''
      echo "INFO: proof workspace dev shell ready"
    '';
  };

  isolation = {
    enable = true;
    slots = [
      5
      7
      8
      9
    ];
    envs = [ ];
    validationInterval = 10;
    maxRuntime = 300;
    startupWait = 30;
    logsDir = "/tmp/${project.id}-isolation";
    keepLogsOnSuccess = false;
    keepLogsOnFailure = true;
    maxParallel = 12;
    useDeps = false;
    run = {
      taskId = "task.test.isolation.probe";
      args = [ ];
    };
    validate = {
      kind = "runApp";
      app = "validate-env";
    };
    runEnv = { };
    setupActions = [ ];
    cleanupActions = [ ];
  };
}
