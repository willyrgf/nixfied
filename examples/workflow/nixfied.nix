{ adapters, ... }:
{
  imports = [ adapters.synthetic ];

  nixfied.project.projectId = "workflow-example";
  nixfied.project.name = "Workflow Example";
  nixfied.codebases.main.logicalRoot = ".";
  nixfied.placement.ports.base = 24680;

  # A bounded workflow: require the synthetic service ready, then run the smoke
  # task twice with a dependency edge between the nodes.
  nixfied.workflows.pipeline = {
    servicesRequired = [ "synthetic" ];
    nodes = {
      probe = {
        taskId = "smoke";
        dependsOn = [ ];
      };
      verify = {
        taskId = "smoke";
        dependsOn = [ "probe" ];
      };
    };
  };
}
