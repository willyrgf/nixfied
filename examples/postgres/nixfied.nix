{ adapters, ... }:
{
  imports = [ adapters.postgres ];

  nixfied.project.projectId = "postgres-example";
  nixfied.project.name = "Postgres Example";
  nixfied.codebases.main.logicalRoot = ".";

  # A dedicated candidate window keeps the example from contending with other
  # examples/proofs for the default port range.
  nixfied.placement.ports.base = 39580;
}
