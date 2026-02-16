{
  project,
  appApi,
}:

rec {
  mkEnvDocProjectEnv = defaultEnv: {
    name = project.envVar;
    description = "Environment name (set to ${defaultEnv} by default for this command)";
  };

  mkEnvDocSlot = {
    name = project.slotVar;
    description = "Slot number (0-9)";
  };

  mkPlaceholderScript = message: ''
    echo "${message}"
    exit 0
  '';

  mkCommand =
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
      class ? "typed",
      env ? { },
      useDeps ? true,
      script ? "",
      category ? "core",
      idempotent ? (class != "passthrough"),
      outputsMode ? "text",
      failureCodes ? appApi.failureProfiles.script,
    }:
    {
      inherit description env useDeps script;
      api = appApi.mkCommandApi {
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
          contractArgs
          contractEnv
          ;
        class = class;
        env = envDocs;
      };
    };

  mkPlaceholderCommand =
    {
      name,
      description,
      details,
      message,
      envDefault ? null,
      includeSlot ? false,
      summary ? description,
      usage ? [ "nix run .#${name}" ],
      examples ? usage,
      args ? [ ],
      class ? "typed",
      category ? "core",
      idempotent ? true,
      outputsMode ? "text",
      failureCodes ? appApi.failureProfiles.script,
    }:
    let
      envDocs =
        (if envDefault == null then [ ] else [ (mkEnvDocProjectEnv envDefault) ])
        ++ (if includeSlot then [ mkEnvDocSlot ] else [ ]);
      env =
        if envDefault == null then
          { }
        else
          {
            "${project.envVar}" = envDefault;
          };
    in
    mkCommand {
      inherit
        name
        description
        summary
        details
        usage
        examples
        args
        class
        env
        category
        idempotent
        outputsMode
        failureCodes
        ;
      inherit envDocs;
      script = mkPlaceholderScript message;
    };

  inherit (appApi) arg env failureProfiles;
}
