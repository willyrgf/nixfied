_:
{
  resolved,
  ...
}:
let
  services = resolved.services or { };
  excludedServices = resolved.graph.excludedServices or [ ];
  listUtils = import ../framework/core/list-utils.nix;
  inherit (listUtils) uniqueSorted;

  names = builtins.sort builtins.lessThan (
    builtins.filter (name: !(builtins.elem name excludedServices)) (builtins.attrNames services)
  );

  sourceKeysFor =
    serviceCfg:
    uniqueSorted ((serviceCfg.sourceKeys or [ ]) ++ builtins.attrNames (serviceCfg.sources or { }));

  probeModesFor = serviceCfg: uniqueSorted (builtins.attrNames (serviceCfg.checks or { }));
in
builtins.listToAttrs (
  map (
    name:
    let
      serviceCfg = services.${name};
    in
    {
      name = "service.${name}";
      value = {
        id = "service.${name}";
        inherit name;
        enable = serviceCfg.enable or false;
        config = {
          dataDirName = serviceCfg.dataDirName or name;
          defaultSource = serviceCfg.defaultSource or "";
          sourceKeys = sourceKeysFor serviceCfg;
          probeModes = probeModesFor serviceCfg;
        };
      };
    }
  ) names
)
