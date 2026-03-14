{
  pkgs,
  ...
}:
let
  shellHelpers = import ./lib/shell-helpers.nix { inherit pkgs; };
  frameworkLib = import ../../nixfied/framework/core {
    inherit pkgs;
    system = pkgs.system;
  };
  slotsStub = {
    getSlotInfoJson = pkgs.writeShellScript "service-probe-overrides-slot-info-json" ''
      printf '{"slot":"%s","env":"%s","ports":{"HTTP_PORT":%s,"HTTPS_PORT":%s},"directories":{"run":"%s","log":"%s","config":"%s"}}\n' \
        "''${SLOT:-0}" \
        "''${ENV:-dev}" \
        "''${HTTP_PORT:-27100}" \
        "''${HTTPS_PORT:-27101}" \
        "''${RUN_DIR:-/tmp}" \
        "''${LOG_DIR:-/tmp}" \
        "''${CONFIG_DIR:-/tmp}"
    '';
    getServiceDir = name: "\${SERVICE_ROOT}/${name}";
    portVarName =
      key:
      if key == "http" then
        "HTTP_PORT"
      else if key == "https" then
        "HTTPS_PORT"
      else
        throw "unsupported port key ${key}";
  };

  compiled = frameworkLib.mkNixfied {
    projectRoot = ../..;
    projectModules = [ ../../nixfied/project/module.nix ];
    extraModules = [
      (
        { lib, ... }:
        {
          nixfied.services.postgres.enable = lib.mkForce false;
          nixfied.services.nginx.enable = lib.mkForce true;
          nixfied.services.minio.enable = lib.mkForce false;
          nixfied.services.reth.enable = lib.mkForce false;
          nixfied.services.helios.enable = lib.mkForce false;

          nixfied.services.nginx.probes = {
            health = {
              strategy = "replace";
              steps = [
                {
                  kind = "exec";
                  command = ''
                    test "$NIXFIED_PROBE_MODE" = "health" &&
                    test "$NIXFIED_PROBE_SERVICE" = "nginx" &&
                    test "$NIXFIED_PROBE_SOURCE" = "nixpkgs" &&
                    test -n "$NIXFIED_PROBE_HTTP_PORT" &&
                    test -n "$NIXFIED_PROBE_HTTPS_PORT"
                  '';
                }
              ];
            };

            ready = {
              strategy = "replace";
              wait = {
                enabled = true;
                timeoutSeconds = 3;
                intervalSeconds = 1;
              };
              steps = [
                {
                  kind = "exec";
                  command = ''
                    count_file="''${TMPDIR:?}/nixfied-nginx-ready-count"
                    count="$(${pkgs.coreutils}/bin/cat "$count_file" 2>/dev/null || echo 0)"
                    count=$((count + 1))
                    printf '%s' "$count" > "$count_file"
                    test "$NIXFIED_PROBE_MODE" = "ready" &&
                    test "$NIXFIED_PROBE_SERVICE" = "nginx" &&
                    test "$NIXFIED_PROBE_SOURCE" = "nixpkgs" &&
                    test -n "$NIXFIED_PROBE_HTTP_PORT" &&
                    test -n "$NIXFIED_PROBE_HTTPS_PORT" &&
                    [ "$count" -ge 2 ]
                  '';
                }
              ];
            };
          };

          nixfied.runtime.ports = lib.mkForce {
            http = 27100;
            https = 27101;
            minioApi = 27102;
            minioConsole = 27103;
            postgres = 27104;
            rethHttp = 27105;
            rethWs = 27106;
            rethAuth = 27107;
            heliosRpc = 27108;
          };
        }
      )
    ];
    localOverrides = [ ];
  };

  directProject = {
    services.nginx = {
      enable = true;
      config = compiled.services."service.nginx".config;
    };
  };

  nginxService = import ../../nixfied/framework/runtime/services/nginx/default.nix {
    inherit pkgs;
    project = directProject;
    slots = slotsStub;
  };

  aggregateHealth = compiled.apps.health.program;
  aggregateReady = compiled.apps.ready.program;
  directHealth = nginxService.health;
  directReady = nginxService.ready;
in
pkgs.runCommand "service-probe-overrides-smoke" { } ''
  set -euo pipefail
  ${shellHelpers.shellPrelude}

  aggregate_health_log="$TMPDIR/aggregate-health.log"
  direct_health_log="$TMPDIR/direct-health.log"
  aggregate_ready_log="$TMPDIR/aggregate-ready.log"
  direct_ready_log="$TMPDIR/direct-ready.log"
  ready_count_file="$TMPDIR/nixfied-nginx-ready-count"

  export REGISTRY_ROOT="$TMPDIR/registry"
  export CI_ARTIFACTS_ROOT="$TMPDIR/artifacts"
  export SLOT=0 ENV=dev HTTP_PORT=27100 HTTPS_PORT=27101
  export SERVICE_ROOT="$TMPDIR/service-root" RUN_DIR="$TMPDIR/run" LOG_DIR="$TMPDIR/log" CONFIG_DIR="$TMPDIR/config"
  mkdir -p "$REGISTRY_ROOT" "$CI_ARTIFACTS_ROOT" "$SERVICE_ROOT" "$RUN_DIR" "$LOG_DIR" "$CONFIG_DIR"

  "${aggregateHealth}" > "$aggregate_health_log" 2>&1 || {
    cat "$aggregate_health_log" >&2
    fail "aggregate health should succeed with override probes"
  }
  require_contains "$aggregate_health_log" "INFO: checking nginx health source=nixpkgs kind=exec"
  require_contains "$aggregate_health_log" "OK: nginx healthy"
  require_contains "$aggregate_health_log" "OK: health checks passed services=1"

  "${directHealth}" > "$direct_health_log" 2>&1 || {
    cat "$direct_health_log" >&2
    fail "direct nginx health should succeed with override probes"
  }
  require_contains "$direct_health_log" "INFO: checking nginx health source=nixpkgs kind=exec"
  require_contains "$direct_health_log" "OK: nginx healthy"

  rm -f "$ready_count_file"
  if "${aggregateReady}" > "$aggregate_ready_log" 2>&1; then
    fail "aggregate ready should remain one-shot when ready.wait is configured"
  fi
  require_contains "$aggregate_ready_log" "INFO: checking nginx readiness source=nixpkgs kind=exec"
  require_contains "$aggregate_ready_log" "ERROR: nginx not ready"
  require_not_contains "$aggregate_ready_log" "OK: readiness checks passed"

  rm -f "$ready_count_file"
  "${directReady}" > "$direct_ready_log" 2>&1 || {
    cat "$direct_ready_log" >&2
    fail "direct nginx ready should succeed with override probes"
  }
  require_contains "$direct_ready_log" "INFO: checking nginx readiness source=nixpkgs kind=exec"
  require_contains "$direct_ready_log" "OK: nginx ready"
  ready_attempts="$(read_trimmed_file "$ready_count_file")"
  if [ "$ready_attempts" -lt 2 ]; then
    fail "expected direct ready wait loop to retry, got $ready_attempts attempts"
  fi

  echo "OK: canonical probe overrides drive aggregate and direct service operations" > "$out"
''
