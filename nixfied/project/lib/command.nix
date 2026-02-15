{
  project,
  appApi,
}:

rec {
  mkEnvDocProjectEnv =
    defaultEnv: {
      name = project.envVar;
      description = "Environment name (set to ${defaultEnv} by default for this command)";
    };

  mkEnvDocSlot = {
    name = project.slotVar;
    description = "Slot number (0-9)";
  };

  mkPlaceholderScript =
    message: ''
      echo "${message}"
      exit 0
    '';

  mkProjectTypedCommand =
    {
      name,
      description,
      summary ? description,
      details ? "",
      usage ? [ "nix run .#${name}" ],
      examples ? usage,
      args ? [ ],
      envDocs ? [ ],
      contractArgs ? null,
      contractEnv ? null,
      outputsKeys ? [ ],
      env ? { },
      useDeps ? true,
      script ? "",
      category ? "core",
      idempotent ? true,
      outputsMode ? "text",
      failureCodes ? appApi.failureProfiles.script,
    }:
    {
      inherit description env useDeps script;
      api = appApi.mkTypedCommandApi {
        inherit
          name
          summary
          details
          usage
          examples
          args
          category
          idempotent
          outputsMode
          failureCodes
          outputsKeys
          ;
        env = envDocs;
        contractArgs = contractArgs;
        contractEnv = contractEnv;
      };
    };

  mkProjectPassthroughCommand =
    {
      name,
      description,
      summary ? description,
      details ? "",
      usage ? [ "nix run .#${name}" ],
      examples ? usage,
      args ? [ ],
      envDocs ? [ ],
      contractArgs ? null,
      contractEnv ? null,
      outputsKeys ? [ ],
      env ? { },
      useDeps ? true,
      script ? "",
      category ? "core",
      idempotent ? false,
      outputsMode ? "text",
      failureCodes ? appApi.failureProfiles.script,
    }:
    {
      inherit description env useDeps script;
      api = appApi.mkPassthroughCommandApi {
        inherit
          name
          summary
          details
          usage
          examples
          args
          category
          idempotent
          outputsMode
          failureCodes
          outputsKeys
          ;
        env = envDocs;
        contractArgs = contractArgs;
        contractEnv = contractEnv;
      };
    };

  mkProjectJsonCommand =
    {
      name,
      description,
      summary ? description,
      details ? "",
      usage ? [ "nix run .#${name}" ],
      examples ? usage,
      args ? [ ],
      envDocs ? [ ],
      contractArgs ? null,
      contractEnv ? null,
      outputsKeys ? [ ],
      env ? { },
      useDeps ? true,
      script ? "",
      category ? "core",
      idempotent ? true,
      failureCodes ? appApi.failureProfiles.script,
    }:
    {
      inherit description env useDeps script;
      api = appApi.mkJsonCommandApi {
        inherit
          name
          summary
          details
          usage
          examples
          args
          category
          idempotent
          failureCodes
          outputsKeys
          ;
        env = envDocs;
        contractArgs = contractArgs;
        contractEnv = contractEnv;
      };
    };

  mkProjectBatchRunnerCommand =
    {
      name,
      description,
      summary ? description,
      details ? "",
      usage ? [ "nix run .#${name}" ],
      examples ? usage,
      args ? [ ],
      envDocs ? [ ],
      contractArgs ? null,
      contractEnv ? null,
      outputsKeys ? [ ],
      env ? { },
      useDeps ? true,
      script ? "",
      category ? "core",
      idempotent ? false,
      outputsMode ? "text",
      failureCodes ? appApi.failureProfiles.script,
    }:
    {
      inherit description env useDeps script;
      api = appApi.mkBatchRunnerCommandApi {
        inherit
          name
          summary
          details
          usage
          examples
          args
          category
          idempotent
          outputsMode
          failureCodes
          outputsKeys
          ;
        env = envDocs;
        contractArgs = contractArgs;
        contractEnv = contractEnv;
      };
    };

  # Backward-compatible alias for existing templates.
  mkProjectCommand = args: mkProjectTypedCommand args;

  inherit (appApi) arg env failureProfiles;
}
