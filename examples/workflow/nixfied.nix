{ adapters, ... }:
{
  imports = [ adapters.synthetic ];

  nixfied.project.projectId = "workflow-example";
  nixfied.project.name = "Workflow Example";
  nixfied.codebases.main.logicalRoot = ".";
  nixfied.placement.ports.base = 24680;

  # A bounded composite: run the smoke task twice with a dependency edge
  # between the steps. Referencing the same leaf twice is legal — each step
  # leaves its own evidence under its step path (pipeline.probe,
  # pipeline.verify).
  nixfied.tasks.pipeline = {
    kind = "composite";
    steps = {
      probe = {
        task = "smoke";
      };
      verify = {
        task = "smoke";
        dependsOn = [ "probe" ];
      };
    };
  };

}
