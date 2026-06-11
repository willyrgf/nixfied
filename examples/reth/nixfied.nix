{ adapters, ... }:
{
  imports = [ adapters.reth ];

  nixfied.project.projectId = "reth-example";
  nixfied.project.name = "Reth Example";
  nixfied.codebases.main.logicalRoot = ".";

  # A dedicated candidate window. reth binds four modelled endpoints (http, ws,
  # authrpc, p2p), so the planner reserves a four-port block from this window; the
  # default window size (11) leaves ample headroom.
  nixfied.placement.ports.base = 25080;
}
