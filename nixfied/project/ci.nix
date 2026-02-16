{
  catalog ? import ./catalog.nix { inherit commandLib project; },
  commandLib,
  project,
  ...
}:

{
  commands.ci = catalog.ci;
  ci = catalog.ciConfig;
}
