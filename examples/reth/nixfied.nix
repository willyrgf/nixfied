{ adapters, ... }:
{
  imports = [ adapters.reth ];

  nixfied.project.projectId = "reth-example";
  nixfied.project.name = "Reth Example";
  nixfied.codebases.main.logicalRoot = ".";

  # A dedicated candidate window. The peerless dev node binds three modelled
  # endpoints (http, ws, authrpc), so the planner reserves a three-port block; the
  # default window size (11) leaves ample headroom.
  nixfied.placement.ports.base = 25080;
}
