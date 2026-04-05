{
  core = import ./core.nix;
  machineOutputs = import ./machine-outputs.nix;
  runtime = import ./runtime.nix;
  serviceSets = import ./service-sets.nix;
  serviceDefinitions = import ./service-definitions.nix;
  tasks = import ./tasks.nix;
  workflows = import ./workflows.nix;
  operations = import ./operations.nix;
  services = import ./services;

  profiles = {
    webapp = import ./profiles/webapp.nix;
  };
}
