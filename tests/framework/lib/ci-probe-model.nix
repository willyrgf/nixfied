{
  pkgs,
  model,
  disableEphemeralWorkflows ? [ ],
}:
let
  probeQualityPkg = pkgs.writeShellScriptBin "ci-quality-probe" ''
    set -euo pipefail
    artifacts_dir="''${CI_ARTIFACTS_DIR:-$REGISTRY_ROOT/artifacts/manual}"
    mkdir -p "$artifacts_dir"
    touch "$artifacts_dir/quality.log"
    echo "OK: probe ci quality step complete"
  '';

  disableEphemeral =
    workflow:
    workflow
    // {
      execution = workflow.execution // {
        ephemeral = (workflow.execution.ephemeral or { }) // {
          enable = false;
        };
      };
    };
in
model
// {
  tasks = model.tasks // {
    "task.ci.quality" = model.tasks."task.ci.quality" // {
      summary = "Probe quality checks";
      description = "Probe quality step used by workflow smoke tests that validate routing instead of nested Nix evaluation.";
      runner = {
        type = "derivation";
        package = probeQualityPkg;
        command = "ci-quality-probe";
        workflowId = null;
      };
    };
  };

  workflows =
    model.workflows
    // builtins.listToAttrs (
      map (workflowId: {
        name = workflowId;
        value = disableEphemeral model.workflows.${workflowId};
      }) disableEphemeralWorkflows
    );
}
