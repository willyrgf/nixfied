{
  catalog ? import ./catalog.nix { inherit commandLib project; },
  commandLib,
  project,
  ...
}:

{
  commands.build = catalog.build;
}
