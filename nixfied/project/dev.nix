{
  catalog ? import ./catalog.nix { inherit commandLib project; },
  commandLib,
  project,
  ...
}:

{
  commands.dev = catalog.dev;
}
