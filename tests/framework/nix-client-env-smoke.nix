{
  pkgs,
  model,
  services,
}:
let
  runtimeFixture = import ./lib/runtime-fixture.nix { inherit pkgs; };
  shellHelpers = import ./lib/shell-helpers.nix { inherit pkgs; };
  baseTask = model.tasks."task.ci.quality";
  probeTaskId = "task.test.nix-client-env.probe";
  probeTask = baseTask // {
    id = probeTaskId;
    summary = "nix client env probe";
    description = "Prints Nix client env vars inside the runtime sandbox.";
    runner = {
      type = "shell";
      command = ''
        set -euo pipefail
        echo "INFO: sandbox_nix_user_conf_files=''${NIX_USER_CONF_FILES:-}"
        echo "INFO: sandbox_nix_config=''${NIX_CONFIG:-}"
        echo "INFO: sandbox_nix_ssl_cert_file=''${NIX_SSL_CERT_FILE:-}"
        echo "OK: nix client env probe complete"
      '';
      package = null;
      workflowId = null;
    };
  };
  probeModel = runtimeFixture.withCompiledExecution (
    model
    // {
      tasks = model.tasks // {
        ${probeTaskId} = probeTask;
      };
    }
  );
  probeExecutionTask = probeModel.compiled.execution.tasks.byId.${probeTaskId};
  inherit (probeExecutionTask) runtimePlanShell;
  envSandboxShell = import ../../nixfied/framework/runtime/env-sandbox.nix {
    inherit
      pkgs
      services
      ;
    model = probeModel;
    projectRoot = ../..;
  };
in
pkgs.runCommand "nix-client-env-smoke" { } ''
  set -euo pipefail
  ${shellHelpers.shellPrelude}
  ${envSandboxShell}

  mkdir -p "$TMPDIR/host-home"
  touch "$TMPDIR/nix-client.conf" "$TMPDIR/nix-client-cert.pem"
  nix_config_value='experimental-features = nix-command flakes'

  PROJECT_ENV=prod \
  NIX_ENV=3 \
  HOME="$TMPDIR/host-home" \
  NIX_USER_CONF_FILES="$TMPDIR/nix-client.conf" \
  NIX_CONFIG="$nix_config_value" \
  NIX_SSL_CERT_FILE="$TMPDIR/nix-client-cert.pem" \
    run_in_sandbox_runtime \
      ${pkgs.lib.escapeShellArg runtimePlanShell} \
      ${pkgs.lib.escapeShellArg probeTask.runner.command} > "$TMPDIR/probe.out" 2>&1 || {
      cat "$TMPDIR/probe.out"
      fail "expected env sandbox to preserve host Nix client env"
    }

  require_contains "$TMPDIR/probe.out" "INFO: sandbox_nix_user_conf_files=$TMPDIR/nix-client.conf"
  require_contains "$TMPDIR/probe.out" "INFO: sandbox_nix_config=$nix_config_value"
  require_contains "$TMPDIR/probe.out" "INFO: sandbox_nix_ssl_cert_file=$TMPDIR/nix-client-cert.pem"
  require_contains "$TMPDIR/probe.out" "OK: nix client env probe complete"

  echo "OK: env sandbox preserves host Nix client env without executor indirection" > "$out"
''
