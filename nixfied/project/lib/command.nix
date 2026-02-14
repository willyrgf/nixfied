{ project }:

{
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

  mkProjectCommand =
    {
      name,
      description,
      summary ? description,
      details ? "",
      usage ? [ "nix run .#${name}" ],
      examples ? usage,
      args ? [ ],
      envDocs ? [ ],
      env ? { },
      useDeps ? true,
      script ? "",
      category ? "core",
    }:
    {
      inherit description env useDeps script;
      api =
        ({ version = 1; inherit summary details usage examples category; })
        // (if args != [ ] then { inherit args; } else { })
        // (if envDocs != [ ] then { env = envDocs; } else { });
    };
}
