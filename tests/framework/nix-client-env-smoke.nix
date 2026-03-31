{
  pkgs,
  model,
  services,
  serviceDefinitions,
  registry,
}:
let
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

  probeModel = model // {
    tasks = model.tasks // {
      ${probeTaskId} = probeTask;
    };
  };

  harness = import ./lib/harness.nix {
    inherit
      pkgs
      registry
      ;
    model = probeModel;
    inherit
      services
      serviceDefinitions
      ;
    projectRoot = ../..;
  };
in
pkgs.runCommand "nix-client-env-smoke" { } ''
  set -euo pipefail
  ${harness.shellPrelude}

  EXECUTOR="${harness.executor}/bin/nixfied-executor"

  mkdir -p "$TMPDIR/registry" "$TMPDIR/artifacts" "$TMPDIR/host-home"
  touch "$TMPDIR/nix-client.conf" "$TMPDIR/nix-client-cert.pem"
  nix_config_value='experimental-features = nix-command flakes'

  PROJECT_ENV=prod \
  NIX_ENV=3 \
  HOME="$TMPDIR/host-home" \
  REGISTRY_ROOT="$TMPDIR/registry" \
  CI_ARTIFACTS_DIR="$TMPDIR/artifacts" \
  NIX_USER_CONF_FILES="$TMPDIR/nix-client.conf" \
  NIX_CONFIG="$nix_config_value" \
  NIX_SSL_CERT_FILE="$TMPDIR/nix-client-cert.pem" \
    "$EXECUTOR" run-task "${probeTaskId}" > "$TMPDIR/probe.out" 2>&1

  require_contains "$TMPDIR/probe.out" "INFO: sandbox_nix_user_conf_files=$TMPDIR/nix-client.conf"
  require_contains "$TMPDIR/probe.out" "INFO: sandbox_nix_config=$nix_config_value"
  require_contains "$TMPDIR/probe.out" "INFO: sandbox_nix_ssl_cert_file=$TMPDIR/nix-client-cert.pem"
  require_contains "$TMPDIR/probe.out" "OK: nix client env probe complete"

  echo "OK: runtime sandbox preserves host Nix client env" > "$out"
''
