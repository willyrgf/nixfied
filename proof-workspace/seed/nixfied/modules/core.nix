{
  lib,
  config,
  ...
}:
let
  t = lib.types;
  moduleContracts = import ./contracts.nix { inherit lib; };
  contractDefinitionType = moduleContracts.contractOptions.contractDefinition;
  configuredServiceNames = builtins.sort builtins.lessThan (
    builtins.attrNames (config.nixfied.services or { })
  );
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
in
{
  imports = [
    ./machine-outputs.nix
    ./service-definitions.nix
    ./service-sets.nix
    ./state.nix
    ./runtime.nix
    ./tasks.nix
    ./workflows.nix
    ./operations.nix
  ];

  options.nixfied = {
    identity = {
      projectId = lib.mkOption {
        type = t.str;
        default = "nixfied-project";
        description = "Stable project identity used in model and state paths.";
      };

      projectName = lib.mkOption {
        type = t.str;
        default = "Nixfied Project";
      };

      description = lib.mkOption {
        type = t.str;
        default = "Model-driven Nixfied project";
      };
    };

    tooling = {
      runtimePackages = lib.mkOption {
        type = t.listOf t.package;
        default = [ ];
      };

      devShellPackages = lib.mkOption {
        type = t.listOf t.package;
        default = [ ];
      };

      devShellHook = lib.mkOption {
        type = t.lines;
        default = ''
          echo "INFO: nixfied dev shell ready"
        '';
      };
    };

    packages = lib.mkOption {
      type = t.attrsOf t.package;
      default = { };
    };

    contracts = {
      version = lib.mkOption {
        type = t.int;
        default = 1;
        description = "Version for the compiled contract bundle.";
      };

      definitions = lib.mkOption {
        type = t.attrsOf contractDefinitionType;
        default = { };
        description = "Project-owned machine contract definitions compiled into the shared contract bundle.";
      };
    };

    graph = {
      excludedServices = lib.mkOption {
        type = t.listOf t.str;
        default = [ ];
        apply = validateServiceNames "nixfied.graph.excludedServices";
        description = "Pure graph-time service exclusions applied before project service projection.";
      };
    };
  };
}
