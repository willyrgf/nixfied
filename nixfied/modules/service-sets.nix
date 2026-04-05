{
  lib,
  config,
  ...
}:
let
  t = lib.types;
  statePolicyOptions = import ./lib/state-policy-options.nix { inherit lib; };
  serviceRequirementType = t.str;

  configuredServices = config.nixfied.services or { };
  configuredServiceNames = builtins.sort builtins.lessThan (builtins.attrNames configuredServices);
  validateServiceNames =
    context: names:
    let
      unknown = builtins.filter (name: !(builtins.elem name configuredServiceNames)) (
        builtins.sort builtins.lessThan names
      );
    in
    if unknown == [ ] then
      names
    else
      throw ''
        ${context} references unknown services: ${builtins.concatStringsSep ", " unknown}
        known services: ${builtins.concatStringsSep ", " configuredServiceNames}
      '';
  excludedServices = config.nixfied.graph.excludedServices or [ ];
  enabledServiceNames = builtins.filter (
    serviceName:
    (configuredServices.${serviceName}.enable or false) && !(builtins.elem serviceName excludedServices)
  ) (builtins.sort builtins.lessThan (builtins.attrNames configuredServices));

  serviceSetType = t.submodule (
    { name, ... }:
    {
      options = {
        id = lib.mkOption {
          type = t.str;
          default = "service-set.${name}";
        };

        summary = lib.mkOption {
          type = t.str;
          default = "Service set ${name}";
        };

        description = lib.mkOption {
          type = t.str;
          default = "";
        };

        services = {
          required = lib.mkOption {
            type = t.listOf serviceRequirementType;
            default = [ ];
            apply = validateServiceNames "nixfied.serviceSets.${name}.services.required";
          };

          optional = lib.mkOption {
            type = t.listOf serviceRequirementType;
            default = [ ];
            apply = validateServiceNames "nixfied.serviceSets.${name}.services.optional";
          };
        };

        defaultOperation = lib.mkOption {
          type = t.enum [
            "health"
            "ready"
          ];
          default = "health";
        };

        state = {
          policy = lib.mkOption {
            type = t.nullOr (t.submodule { options = statePolicyOptions; });
            default = null;
          };
        };

        export = {
          defaultFormat = lib.mkOption {
            type = t.enum [
              "json"
              "env"
            ];
            default = "json";
          };
        };

        failureLogs = {
          capture = lib.mkOption {
            type = t.bool;
            default = true;
          };

          tailLines = lib.mkOption {
            type = t.ints.unsigned;
            default = 40;
          };
        };

        ownerFile = lib.mkOption {
          type = t.nullOr t.str;
          default = null;
        };
      };
    }
  );

in
{
  options.nixfied.serviceSets = lib.mkOption {
    type = t.attrsOf serviceSetType;
    default = { };
  };

  config = {
    nixfied.serviceSets.default = lib.mkDefault {
      id = "service-set.default";
      summary = "Default service set";
      description = "All enabled project services grouped behind stable lifecycle surfaces.";
      services.required = enabledServiceNames;
      services.optional = [ ];
      defaultOperation = "health";
      ownerFile = "nixfied/modules/service-sets.nix";
    };

  };
}
