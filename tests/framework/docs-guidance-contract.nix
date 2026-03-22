{ pkgs, model }:
let
  readme = builtins.readFile ./README.md;
  workflowReuse = builtins.readFile ./WORKFLOW_REUSE.md;
  serviceLifecycle = builtins.readFile ./SERVICE_LIFECYCLE_API.md;
  docsText = builtins.concatStringsSep "\n" model.views.docs.lines;
  readyApp = model.apps.ready;
  healthApp = model.apps.health;
  readyExamples = readyApp.examples or [ ];
  healthExamples = healthApp.examples or [ ];
  hasExample = examples: needle: builtins.elem needle examples;
in
assert pkgs.lib.hasInfix "## Reuse Guides" readme;
assert pkgs.lib.hasInfix "WORKFLOW_REUSE.md" readme;
assert pkgs.lib.hasInfix "SERVICE_LIFECYCLE_API.md" readme;
assert pkgs.lib.hasInfix "## Current Semantics" workflowReuse;
assert pkgs.lib.hasInfix "strict machine-output stdout contract" workflowReuse;
assert pkgs.lib.hasInfix "workflow unit closure service set" workflowReuse;
assert pkgs.lib.hasInfix "## Public Service Matrix" serviceLifecycle;
assert pkgs.lib.hasInfix "It does not expose `ready`" serviceLifecycle;
assert pkgs.lib.hasInfix "Workflow `preRun` / `postRun` probe phases scope the aggregate ops"
  serviceLifecycle;
assert pkgs.lib.hasInfix "## Current Workflow Semantics" docsText;
assert pkgs.lib.hasInfix "Workflows forward one shared passthrough argv" docsText;
assert pkgs.lib.hasInfix "## Service Lifecycle Review" docsText;
assert pkgs.lib.hasInfix "Supervisor is a separate runtime surface and does not provide ready."
  docsText;
assert hasExample readyExamples "nix run .#ready -- --service postgres";
assert hasExample readyExamples "nix run .#ready -- --service helios --source real";
assert hasExample healthExamples "nix run .#health -- --service postgres";
assert hasExample healthExamples "nix run .#health -- --service helios --source real";
pkgs.runCommand "docs-guidance-contract" { } ''
  echo "OK: workflow guidance and service lifecycle review remain discoverable in docs and help" > "$out"
''
