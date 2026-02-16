{
  catalog ? import ./catalog.nix { inherit commandLib project; },
  commandLib,
  project,
  ...
}:

{
  commands.test = catalog.test;
}
