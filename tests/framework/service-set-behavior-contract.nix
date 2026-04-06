{ pkgs }:
let
  frameworkLib = import ../../nixfied/framework/core {
    inherit pkgs;
    inherit (pkgs) system;
  };
  shellHelpers = import ./lib/shell-helpers.nix { inherit pkgs; };

  workflowId = "workflow.test.service-set.phase";
  taskId = "task.test.service-set.phase.body";

  workflowModule = {
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
      summary = "Workflow phase service-set behavior contract";
      description = "Exercises workflow service-set phase ordering through the public runtime app.";
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
  };

  frameworkOutputs = frameworkLib.mkFlakeOutputs {
    projectRoot = ../..;
    projectModules = [ ../../nixfied/project/module.nix ];
    extraModules = [ workflowModule ];
    localOverrides = [ ];
  };
in
pkgs.runCommand "service-set-behavior-contract" { } ''
  set -euo pipefail
  ${shellHelpers.shellPrelude}

  RUN_WORKFLOW_APP="${frameworkOutputs.apps."run-workflow".program}"

  export REGISTRY_ROOT="$TMPDIR/registry"
  export CI_ARTIFACTS_ROOT="$TMPDIR/ci-artifacts"
  export NIXFIED_RUNTIME_DIR_SCOPE_OVERRIDE="$TMPDIR/runtime-scope"
  mkdir -p "$REGISTRY_ROOT" "$CI_ARTIFACTS_ROOT" "$NIXFIED_RUNTIME_DIR_SCOPE_OVERRIDE"

  "$RUN_WORKFLOW_APP" "${workflowId}" > "$TMPDIR/workflow.out" 2>&1 || {
    cat "$TMPDIR/workflow.out"
    fail "workflow phase service-set contract should succeed"
  }

  require_contains "$TMPDIR/workflow.out" "BODY"
  require_contains "$TMPDIR/workflow.out" "SKIP: service-set phase serviceSet=default operation=status reason=all-services-excluded"

  status_count="$(${pkgs.gnugrep}/bin/grep -c '^SKIP: service-set phase serviceSet=default operation=status reason=all-services-excluded$' "$TMPDIR/workflow.out" || true)"
  if [ "$status_count" -ne 2 ]; then
    cat "$TMPDIR/workflow.out"
    fail "expected two service-set phase status lines"
  fi

  pre_line="$(${pkgs.gnugrep}/bin/grep -n '^SKIP: service-set phase serviceSet=default operation=status reason=all-services-excluded$' "$TMPDIR/workflow.out" | ${pkgs.coreutils}/bin/head -n1 | ${pkgs.gawk}/bin/awk -F: '{print $1}')"
  body_line="$(${pkgs.gnugrep}/bin/grep -n '^BODY$' "$TMPDIR/workflow.out" | ${pkgs.coreutils}/bin/head -n1 | ${pkgs.gawk}/bin/awk -F: '{print $1}')"
  post_line="$(${pkgs.gnugrep}/bin/grep -n '^SKIP: service-set phase serviceSet=default operation=status reason=all-services-excluded$' "$TMPDIR/workflow.out" | ${pkgs.coreutils}/bin/tail -n1 | ${pkgs.gawk}/bin/awk -F: '{print $1}')"

  if [ -z "$pre_line" ] || [ -z "$body_line" ] || [ -z "$post_line" ]; then
    cat "$TMPDIR/workflow.out"
    fail "missing expected workflow phase markers"
  fi

  if [ "$pre_line" -ge "$body_line" ] || [ "$body_line" -ge "$post_line" ]; then
    cat "$TMPDIR/workflow.out"
    fail "workflow phase service-set adapters did not preserve phase ordering"
  fi

  echo "OK: workflow service-set phases stay ordered through the public run-workflow app" > "$out"
''
