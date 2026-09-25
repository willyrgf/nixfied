{ adapters, ... }:
{
  imports = [ adapters.postgres ];

  nixfied.project.projectId = "postgres-example";
  nixfied.project.name = "Postgres Example";
  nixfied.codebases.main.logicalRoot = ".";
  # The database survives sessions; ordinary `clean` refuses, `clean --purge` deletes it.
  nixfied.state.persistence = "persistent";

  # A dedicated candidate window keeps the example from contending with other
  # examples/proofs for the default port range.
  nixfied.placement.ports.base = 24580;
}
