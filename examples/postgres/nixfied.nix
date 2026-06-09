{ adapters, ... }:
{
  imports = [ adapters.postgres ];

  nixfied.project.projectId = "postgres-example";
  nixfied.project.name = "Postgres Example";
  nixfied.codebases.main.logicalRoot = ".";
}
