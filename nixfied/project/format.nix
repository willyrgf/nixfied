{
  catalog ? import ./catalog.nix { inherit commandLib project; },
  commandLib,
  project,
  ...
}:

{
  commands.format = catalog.format;
}
