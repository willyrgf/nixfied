{ adapters, ... }:
{
  imports = [ adapters.reth ];

  nixfied.project.projectId = "reth-example";
  nixfied.project.name = "Reth Example";
  nixfied.codebases.main.logicalRoot = ".";

  # A dedicated candidate window: reth's wrapper derives ws/auth/p2p listeners
  # as http+1/+2/+3, which the planner does not reserve, so the window must not
  # be shared with other examples/services.
  nixfied.placement.ports.base = 25080;
}
