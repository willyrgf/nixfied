{
  pkgs,
  projectRoot,
  registry,
}:
{
  mkApps =
    {
      model,
      frameworkSourceFlakeRef ? null,
      serviceApps ? { },
      serviceHookEnv ? { },
    }:
    import ./dispatcher.nix {
      inherit
        pkgs
        model
        projectRoot
        registry
        frameworkSourceFlakeRef
        serviceApps
        serviceHookEnv
        ;
    };
}
