{
  pkgs,
  projectRoot,
  registry,
}:
{
  mkApps =
    {
      model,
      selectionIndex,
      services,
      runtimeHash ? model.identity.evalHash,
      frameworkSourceFlakeRef ? null,
      taskAppPrograms ? { },
      serviceApps ? { },
      serviceHookEnv ? { },
    }:
    import ./dispatcher.nix {
      inherit
        pkgs
        model
        selectionIndex
        services
        runtimeHash
        projectRoot
        registry
        frameworkSourceFlakeRef
        taskAppPrograms
        serviceApps
        serviceHookEnv
        ;
    };
}
