{
  pkgs,
  registry,
  ...
}:
let
  shellHelpers = import ./lib/shell-helpers.nix { inherit pkgs; };
  frameworkLib = import ../../nixfied/framework/core {
    inherit pkgs;
    inherit (pkgs) system;
  };

  mkCompiled =
    {
      enableNginx,
      enableMinio,
      basePort,
    }:
    frameworkLib.mkNixfied {
      projectRoot = ../..;
      projectModules = [ ../../nixfied/project/module.nix ];
      extraModules = [
        (
          { lib, ... }:
          {
            nixfied.services = {
              postgres.enable = lib.mkForce false;
              nginx.enable = lib.mkForce enableNginx;
              minio.enable = lib.mkForce enableMinio;
              reth.enable = lib.mkForce false;
              helios.enable = lib.mkForce false;
            };

            nixfied.runtime.ports = lib.mkForce {
              http = basePort + 0;
              https = basePort + 1;
              minioApi = basePort + 2;
              minioConsole = basePort + 3;
              postgres = basePort + 4;
              rethHttp = basePort + 5;
              rethWs = basePort + 6;
              rethAuth = basePort + 7;
              heliosRpc = basePort + 8;
            };
          }
        )
      ];
      localOverrides = [ ];
    };

  noneCompiled = mkCompiled {
    enableNginx = false;
    enableMinio = false;
    basePort = 26100;
  };

  nginxCompiled = mkCompiled {
    enableNginx = true;
    enableMinio = false;
    basePort = 26200;
  };

  minioCompiled = mkCompiled {
    enableNginx = false;
    enableMinio = true;
    basePort = 26300;
  };

  bothCompiled = mkCompiled {
    enableNginx = true;
    enableMinio = true;
    basePort = 26400;
  };

  runtimeSlotStride = toString noneCompiled.model.runtime.slot.stride;
  runtimeEnvDevOffset = toString (noneCompiled.model.runtime.env.offsets.dev or 0);
  runtimeEnvTestOffset = toString (noneCompiled.model.runtime.env.offsets.test or 0);

  mkExecutor =
    compiled:
    import ../../nixfied/framework/runtime/executor.nix {
      inherit
        pkgs
        registry
        ;
      inherit (compiled) model;
      inherit (compiled) services;
      projectRoot = ../..;
    };

  noneExecutor = mkExecutor noneCompiled;
  nginxExecutor = mkExecutor nginxCompiled;
  minioExecutor = mkExecutor minioCompiled;
  bothExecutor = mkExecutor bothCompiled;
in
pkgs.runCommand "ready-health-matrix-smoke" { } ''
  set -euo pipefail
  ${shellHelpers.shellPrelude}

  NONE_EXECUTOR="${noneExecutor}/bin/nixfied-executor"
  NGINX_EXECUTOR="${nginxExecutor}/bin/nixfied-executor"
  MINIO_EXECUTOR="${minioExecutor}/bin/nixfied-executor"
  BOTH_EXECUTOR="${bothExecutor}/bin/nixfied-executor"

  offset_for_env() {
    local env_name="$1"
    case "$env_name" in
      dev)
        echo ${runtimeEnvDevOffset}
        ;;
      test)
        echo ${runtimeEnvTestOffset}
        ;;
      *)
        echo "unsupported env $env_name"
        exit 1
        ;;
    esac
  }

  compute_port() {
    local base="$1"
    local slot="$2"
    local env_offset="$3"
    echo $(( base + env_offset + (slot * ${runtimeSlotStride}) ))
  }

  run_matrix_case() {
    local label="$1"
    local executor_bin="$2"
    local mode="$3"
    local base_port="$4"

    local env_name
    local slot
    local env_offset
    local registry_root
    local ready_log
    local health_log
    local ready_rc
    local health_rc
    local expected_port

    for env_name in dev test; do
      for slot in 0 2; do
        env_offset="$(offset_for_env "$env_name")"
        registry_root="$TMPDIR/registry-$label-$env_name-$slot"
        ready_log="$TMPDIR/$label-$env_name-$slot-ready.out"
        health_log="$TMPDIR/$label-$env_name-$slot-health.out"

        mkdir -p "$registry_root"

        set +e
        REGISTRY_ROOT="$registry_root" NIX_ENV="$slot" PROJECT_ENV="$env_name" \
          "$executor_bin" run-task task.ops.ready > "$ready_log" 2>&1
        ready_rc="$?"

        REGISTRY_ROOT="$registry_root" NIX_ENV="$slot" PROJECT_ENV="$env_name" \
          "$executor_bin" run-task task.ops.health > "$health_log" 2>&1
        health_rc="$?"
        set -e

        if [ "$mode" = "none" ]; then
          if [ "$ready_rc" -ne 0 ] || [ "$health_rc" -ne 0 ]; then
            echo "expected none mode to skip with exit code 0 (ready=$ready_rc health=$health_rc)"
            cat "$ready_log"
            cat "$health_log"
            exit 1
          fi
          require_contains "$ready_log" "SKIP: no enabled services for readiness checks"
          require_contains "$health_log" "SKIP: no enabled services for health checks"
        else
          if [ "$ready_rc" -eq 0 ] || [ "$health_rc" -eq 0 ]; then
            echo "expected mode '$mode' to fail without listeners (ready=$ready_rc health=$health_rc)"
            cat "$ready_log"
            cat "$health_log"
            exit 1
          fi

          case "$mode" in
            nginx)
              expected_port="$(compute_port "$base_port" "$slot" "$env_offset")"
              require_contains "$ready_log" "ERROR: nginx not ready port=$expected_port"
              require_contains "$health_log" "ERROR: nginx unhealthy port=$expected_port"
              ;;
            minio)
              expected_port="$(compute_port "$((base_port + 2))" "$slot" "$env_offset")"
              require_contains "$ready_log" "ERROR: minio not ready port=$expected_port"
              require_contains "$health_log" "ERROR: minio unhealthy port=$expected_port"
              ;;
            both)
              nginx_port="$(compute_port "$base_port" "$slot" "$env_offset")"
              minio_port="$(compute_port "$((base_port + 2))" "$slot" "$env_offset")"
              if ! ${pkgs.gnugrep}/bin/grep -Fq "ERROR: nginx not ready port=$nginx_port" "$ready_log" \
                && ! ${pkgs.gnugrep}/bin/grep -Fq "ERROR: minio not ready port=$minio_port" "$ready_log"; then
                cat "$ready_log"
                fail "expected both-mode ready failure for nginx or minio"
              fi
              if ! ${pkgs.gnugrep}/bin/grep -Fq "ERROR: nginx unhealthy port=$nginx_port" "$health_log" \
                && ! ${pkgs.gnugrep}/bin/grep -Fq "ERROR: minio unhealthy port=$minio_port" "$health_log"; then
                cat "$health_log"
                fail "expected both-mode health failure for nginx or minio"
              fi
              ;;
            *)
              echo "unsupported mode '$mode'"
              exit 1
              ;;
          esac
        fi
      done
    done
  }

  run_matrix_case "none" "$NONE_EXECUTOR" "none" 26100
  run_matrix_case "nginx" "$NGINX_EXECUTOR" "nginx" 26200
  run_matrix_case "minio" "$MINIO_EXECUTOR" "minio" 26300
  run_matrix_case "both" "$BOTH_EXECUTOR" "both" 26400

  echo "OK: ready/health commands execute across slot/env and service matrix" > "$out"
''
