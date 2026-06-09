{ adapters, ... }:
{
  imports = [ adapters.synthetic ];

  nixfied.project.projectId = "minimal";
  nixfied.project.name = "Minimal";
  nixfied.codebases.main.logicalRoot = ".";
}
