# Service config builder -- extracts the boilerplate shared by every
# service config.nix: importing service-config.nix, calling
# getProjectServiceConfig, and merging user overrides with defaults.
#
# Usage:
#   import ../service-config-builder.nix {
#     inherit pkgs project;
#     name = "reth";
#     defaults = cfg: {
#       package = cfg.package or null;
#       portKeyHttp = cfg.portKeyHttp or "rethHttp";
#       ...
#     };
#   }
#
# The `defaults` function receives `cfg` (the project service config
# attrset) and must return the final config attrset.  probePlans,
# resolvedEndpoints, and defaultSource are appended automatically.
{
  pkgs,
  project,
  name,
  defaults,
  includeProbes ? true,
}:

let
  serviceConfig = import ../../core/service-config.nix {
    lib = pkgs.lib;
    inherit pkgs;
  };
  cfg = serviceConfig.getProjectServiceConfig {
    inherit project name;
  };
  base = defaults cfg;
  probeAttrs =
    if includeProbes then
      {
        defaultSource = cfg.defaultSource or "";
        probePlans = cfg.resolved.probePlans or cfg.resolved.operationProbes or { };
        resolvedEndpoints = cfg.resolved.endpoints or { };
      }
    else
      { };
in
base // probeAttrs
