{
  pkgs,
  projectRoot,
  registry,
}:
{
  mkApps =
    {
      model,
      serviceApps ? { },
      serviceHookEnv ? { },
    }:
    import ./dispatcher.nix {
      inherit
        pkgs
        model
        projectRoot
        registry
        serviceApps
        serviceHookEnv
        ;
    };
}
