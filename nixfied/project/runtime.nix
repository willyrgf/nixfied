{
  conf,
  project,
  envNames,
  envOffsets,
  nixChecksPkg,
}:
{
  config = {
    nixfied = {
      identity = {
        projectId = project.id;
        projectName = project.name;
        inherit (project) description;
      };

      runtime = {
        slot = {
          var = project.slotVar;
          inherit (conf.slots) default;
          inherit (conf.slots) max;
          inherit (conf.slots) stride;
        };

        env = {
          var = project.envVar;
          names = envNames;
          offsets = envOffsets;
          default = "dev";
        };

        logging = {
          levelDefault = conf.logging.level;
          outputDefault = conf.logging.output;
        };

        inherit (conf) ports;
        ephemeral = {
          copyMode = conf.ephemeral.copyMode or "nix-source";
          includeUntracked = conf.ephemeral.includeUntracked or false;
          excludePatterns =
            conf.ephemeral.excludePatterns or [
              ".git"
              "node_modules"
              ".next"
              "dist"
              ".turbo"
              ".cache"
              "result"
              "result-*"
              "*.log"
              "test-results"
              "coverage"
            ];
          extraDirs = conf.ephemeral.extraDirs or [ ];
          keepFailures = conf.ephemeral.keepFailures or true;
          maxFailedRoots = conf.ephemeral.maxFailedRoots or 8;
          maxFailedRootAgeHours = conf.ephemeral.maxFailedRootAgeHours or 72;
          maxCopyBytes = conf.ephemeral.maxCopyBytes or 0;
          minFreeBytesAfterCopy = conf.ephemeral.minFreeBytesAfterCopy or 0;
          envFileMode = conf.ephemeral.envFileMode or "disabled";
          envFilePath = conf.ephemeral.envFilePath or ".env";
        };
      };

      tooling = {
        inherit (conf.tooling) runtimePackages;
        inherit (conf.tooling) devShellPackages;
        inherit (conf.tooling) devShellHook;
      };

      packages = {
        "nix-checks" = nixChecksPkg;
      };

      operations = {
        enable = true;
        validateEnv.enable = true;
        testIsolation = {
          enable = conf.isolation.enable or true;
          slots =
            conf.isolation.slots or [
              conf.slots.default
            ];
          envs =
            let
              configured = conf.isolation.envs or [ ];
            in
            if configured == [ ] then envNames else configured;
          logsDir = conf.isolation.logsDir or "/tmp/${project.id}-isolation";
          keepLogsOnSuccess = conf.isolation.keepLogsOnSuccess or false;
          keepLogsOnFailure = conf.isolation.keepLogsOnFailure or true;
          maxParallel = conf.isolation.maxParallel or 4;
          runTaskId =
            conf.isolation.run.taskId or (
              let
                configuredApp = conf.isolation.run.app or "ci";
              in
              if configuredApp == "ci" then
                "task.ci"
              else
                throw "ERROR: isolation.run.taskId must be set when isolation.run.app is not 'ci'"
            );
          runApp = conf.isolation.run.app or "ci";
          runArgs = conf.isolation.run.args or [ "--summary" ];
          validateTaskId =
            conf.isolation.validate.taskId or (
              let
                configuredApp = conf.isolation.validate.app or "validate-env";
              in
              if configuredApp == "validate-env" then
                "task.ops.validate-env"
              else
                throw "ERROR: isolation.validate.taskId must be set when isolation.validate.app is not 'validate-env'"
            );
          validateApp = conf.isolation.validate.app or "validate-env";
          runEnv = conf.isolation.runEnv or { };
        };
        ports.enable = true;
        checkPorts.enable = true;
        health.enable = true;
        ready.enable = true;
      };
    };
  };
}
