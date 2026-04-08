{
  conf,
  envNames,
  envOffsets,
}:
{
  config = {
    nixfied = {
      identity = {
        projectId = conf.project.id;
        projectName = conf.project.name;
        inherit (conf.project) description;
      };

      runtime = {
        slot = {
          var = conf.project.slotVar;
          inherit (conf.slots) default;
          inherit (conf.slots) max;
          inherit (conf.slots) stride;
        };

        env = {
          var = conf.project.envVar;
          names = envNames;
          offsets = envOffsets;
          default = "dev";
        };

        logging = {
          levelDefault = conf.logging.level;
          outputDefault = conf.logging.output;
        };

        ports = conf.ports;
      };

      tooling = {
        inherit (conf.tooling) runtimePackages;
        inherit (conf.tooling) devShellPackages;
        inherit (conf.tooling) devShellHook;
      };

      operations = {
        enable = true;
        validateEnv.enable = true;
        testIsolation = {
          enable = conf.isolation.enable;
          slots = conf.isolation.slots;
          envs =
            if conf.isolation.envs == [ ] then envNames else conf.isolation.envs;
          logsDir = conf.isolation.logsDir;
          keepLogsOnSuccess = conf.isolation.keepLogsOnSuccess;
          keepLogsOnFailure = conf.isolation.keepLogsOnFailure;
          maxParallel = conf.isolation.maxParallel;
          runTaskId = conf.isolation.run.taskId;
          runApp = conf.isolation.run.app or "run-task";
          runArgs = conf.isolation.run.args;
          validateTaskId = conf.isolation.validate.taskId or "task.ops.validate-env";
          validateApp = conf.isolation.validate.app or "validate-env";
          runEnv = conf.isolation.runEnv;
        };
        ports.enable = true;
        checkPorts.enable = true;
        health.enable = true;
        ready.enable = true;
      };
    };
  };
}
