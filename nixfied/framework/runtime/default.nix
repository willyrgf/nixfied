{
  pkgs,
  projectRoot,
  registry,
}:
{
  mkApps =
    {
      model,
      services,
      runtimeHash ? model.identity.evalHash,
      frameworkSourceFlakeRef ? null,
      appPrograms ? { },
      serviceSetPrograms ? { },
      serviceHookEnv ? { },
      includeRuntimeControlApps ? true,
    }:
    import ./dispatcher.nix {
      inherit
        pkgs
        model
        services
        runtimeHash
        projectRoot
        registry
        frameworkSourceFlakeRef
        appPrograms
        serviceSetPrograms
        serviceHookEnv
        includeRuntimeControlApps
        ;
    };
}
