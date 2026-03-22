{ pkgs }:
let
  frameworkLib = import ../../nixfied/framework/core {
    inherit pkgs;
    system = pkgs.system;
  };
  shellHelpers = import ./lib/shell-helpers.nix { inherit pkgs; };
  repoRoot = builtins.toString ../..;
  workflowId = "workflow.test.service-set.phase";
  taskId = "task.test.service-set.phase.body";

  frameworkOutputs = frameworkLib.mkFlakeOutputs {
    projectRoot = ../..;
    projectModules = [ ../../nixfied/project/module.nix ];
    extraModules = [
      {
        nixfied.tasks."test.service-set.phase.body" = {
          id = taskId;
          summary = "service-set phase body";
          description = "Emits a marker so workflow phase ordering stays testable.";
          runner.command = ''
            set -euo pipefail
            printf '%s\n' "BODY"
          '';
        };

        nixfied.workflows."test.service-set.phase" = {
          id = workflowId;
          summary = "Workflow phase service-set adapter smoke";
          description = "Exercises preRun and postRun service-set adapters.";
          units.main.taskId = taskId;
          preRun.serviceSets = [
            {
              serviceSetId = "service-set.default";
              operation = "status";
            }
          ];
          postRun = {
            serviceSets = [
              {
                serviceSetId = "service-set.default";
                operation = "status";
              }
            ];
            alwaysRun = true;
          };
        };
      }
    ];
    localOverrides = [ ];
  };
in
pkgs.runCommand "workflow-service-set-adapter-smoke" { } ''
  set -euo pipefail
  ${shellHelpers.shellPrelude}

  RUN_WORKFLOW_APP="${frameworkOutputs.apps."run-workflow".program}"

  export REGISTRY_ROOT="$TMPDIR/registry"
  export CI_ARTIFACTS_ROOT="$TMPDIR/ci-artifacts"
  export NIXFIED_RUNTIME_DIR_SCOPE_OVERRIDE="$TMPDIR/runtime-scope"
  export NIXFIED_FLAKE_ROOT=${pkgs.lib.escapeShellArg repoRoot}
  mkdir -p "$REGISTRY_ROOT" "$CI_ARTIFACTS_ROOT" "$NIXFIED_RUNTIME_DIR_SCOPE_OVERRIDE"
  cd "$NIXFIED_FLAKE_ROOT"

  "$RUN_WORKFLOW_APP" ${pkgs.lib.escapeShellArg workflowId} > "$TMPDIR/workflow.out" 2>&1 || {
    cat "$TMPDIR/workflow.out"
    fail "workflow phase service-set adapter should succeed"
  }

  require_contains "$TMPDIR/workflow.out" "BODY"
  require_contains "$TMPDIR/workflow.out" "OK: service-set default status passed services=0"

  status_count="$(${pkgs.gnugrep}/bin/grep -c '^OK: service-set default status passed services=0$' "$TMPDIR/workflow.out" || true)"
  if [ "$status_count" -ne 2 ]; then
    cat "$TMPDIR/workflow.out"
    fail "expected two service-set phase status lines"
  fi

  pre_line="$(${pkgs.gnugrep}/bin/grep -n '^OK: service-set default status passed services=0$' "$TMPDIR/workflow.out" | ${pkgs.coreutils}/bin/head -n1 | ${pkgs.gawk}/bin/awk -F: '{print $1}')"
  body_line="$(${pkgs.gnugrep}/bin/grep -n '^BODY$' "$TMPDIR/workflow.out" | ${pkgs.coreutils}/bin/head -n1 | ${pkgs.gawk}/bin/awk -F: '{print $1}')"
  post_line="$(${pkgs.gnugrep}/bin/grep -n '^OK: service-set default status passed services=0$' "$TMPDIR/workflow.out" | ${pkgs.coreutils}/bin/tail -n1 | ${pkgs.gawk}/bin/awk -F: '{print $1}')"

  if [ -z "$pre_line" ] || [ -z "$body_line" ] || [ -z "$post_line" ]; then
    cat "$TMPDIR/workflow.out"
    fail "missing expected workflow phase markers"
  fi

  if [ "$pre_line" -ge "$body_line" ] || [ "$body_line" -ge "$post_line" ]; then
    cat "$TMPDIR/workflow.out"
    fail "workflow phase service-set adapters did not preserve phase ordering"
  fi

  echo "OK: workflow phase service-set adapters execute before and after workflow units" > "$out"
''
