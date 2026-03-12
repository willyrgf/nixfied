{
  mkCommandTask,
  pkgs,
  frameworkSourceRevision ? "unknown",
  ownerFile ? "nixfied/project/module.nix",
}:
let
  plainShellLogging = import ../../lib/plain-shell-logging.nix;

  thinWrapperFlake = import ../../install/wrapper-flake.nix {
    frameworkInput = "github:willyrgf/nixfied/dev";
  };

  vendoredWrapperFlake = import ../../install/wrapper-flake.nix {
    vendorPath = "./nixfied";
  };

  vendoredMetadata = ''
    Vendored Framework
    ==================

    This repository vendors the Nixfied framework under `nixfied/`.

    Framework source revision (install/upgrade):
    - ${frameworkSourceRevision}

    Framework source revision workflow:
    - initialized via `framework::install`
    - upgraded via `framework::upgrade` (preserves `nixfied/project/` and `nixfied/local/` by default)

    Framework-owned paths:
    - `flake.nix`, `flake.lock`
    - `nixfied/.framework/`

    User-owned customization paths:
    - `nixfied/project/` (primary command/task/workflow customization surface)
    - `nixfied/local/` (optional extensions)

    Prefer editing `nixfied/project/` and `nixfied/local/` over direct framework internals.
  '';

  frameworkInstallRuntimeInputs = [
    pkgs.coreutils
    pkgs.findutils
    pkgs.gnused
    pkgs.rsync
  ];

  frameworkInstallContractArgs = [
    {
      name = "vendor";
      kind = "flag";
      long = "--vendor";
      description = "Generate a vendored wrapper flake.";
    }
    {
      name = "target";
      kind = "option";
      long = "--target";
      type = "string";
      description = "Output directory for generated wrapper.";
    }
    {
      name = "upgrade";
      kind = "flag";
      long = "--upgrade";
      description = "Upgrade vendored framework files in-place and preserve nixfied/project + nixfied/local.";
    }
    {
      name = "reset-project";
      kind = "flag";
      long = "--reset-project";
      description = "When vendoring, overwrite nixfied/project.";
    }
    {
      name = "reset-local";
      kind = "flag";
      long = "--reset-local";
      description = "When vendoring, overwrite nixfied/local.";
    }
  ];

  frameworkUpgradeContractArgs = [
    {
      name = "vendor";
      kind = "flag";
      long = "--vendor";
      description = "Generate a vendored wrapper flake (default for framework::upgrade).";
    }
    {
      name = "target";
      kind = "option";
      long = "--target";
      type = "string";
      description = "Output directory for generated wrapper.";
    }
    {
      name = "reset-project";
      kind = "flag";
      long = "--reset-project";
      description = "When vendoring, overwrite nixfied/project.";
    }
    {
      name = "reset-local";
      kind = "flag";
      long = "--reset-local";
      description = "When vendoring, overwrite nixfied/local.";
    }
  ];

  mkFrameworkInstallCommand =
    {
      upgradeDefault ? false,
    }:
    ''
            set -euo pipefail

            source_root="${builtins.toString ../../.}"
            repo_root="${builtins.toString ../../../.}"
            target="."
            vendor=${if upgradeDefault then "1" else "0"}
            upgrade=${if upgradeDefault then "1" else "0"}
            reset_project=0
            reset_local=0

            usage() {
              cat <<'EOF'
      ${
        if upgradeDefault then
          ''
            Usage:
              nix run .#framework::upgrade -- --target .
              nix run .#framework::upgrade -- --target . --reset-project
              nix run .#framework::upgrade -- --target . --reset-local

            Upgrade vendored wrapper in-place while preserving nixfied/project and nixfied/local by default.

            Options:
              --vendor          Generate a vendored wrapper flake (default for framework::upgrade).
              --target <path>   Output directory for generated wrapper.
              --reset-project   When vendoring, overwrite nixfied/project.
              --reset-local     When vendoring, overwrite nixfied/local.
              --help, -h        Show this help.
          ''
        else
          ''
            Usage:
              nix run .#framework::install
              nix run .#framework::install -- --vendor
              nix run .#framework::install -- --vendor --target .
              nix run .#framework::install -- --vendor --upgrade --target .

            Install a thin wrapper flake by default, or a vendored wrapper with --vendor.

            Options:
              --vendor          Generate a vendored wrapper flake.
              --target <path>   Output directory for generated wrapper.
              --upgrade         Upgrade vendored framework files in-place and preserve nixfied/project + nixfied/local.
              --reset-project   When vendoring, overwrite nixfied/project.
              --reset-local     When vendoring, overwrite nixfied/local.
              --help, -h        Show this help.
          ''
      }
      EOF
            }

            ${plainShellLogging {
              includeWarn = false;
              includeSkip = false;
              errorToStderr = true;
            }}

            while [ "$#" -gt 0 ]; do
              case "$1" in
                --vendor)
                  vendor=1
                  shift
                  ;;
                --upgrade)
                  upgrade=1
                  vendor=1
                  shift
                  ;;
                --reset-project)
                  reset_project=1
                  shift
                  ;;
                --reset-local)
                  reset_local=1
                  shift
                  ;;
                --target)
                  if [ "$#" -lt 2 ]; then
                    log_error "--target requires a value"
                    exit 2
                  fi
                  target="$2"
                  shift 2
                  ;;
                --help|-h)
                  usage
                  exit 0
                  ;;
                --)
                  shift
                  break
                  ;;
                *)
                  log_error "unknown argument '$1'"
                  exit 2
                  ;;
              esac
            done

            if [ "$#" -gt 0 ]; then
              log_error "unexpected positional arguments: $*"
              exit 2
            fi

            if [ "$vendor" -eq 0 ] && { [ "$reset_project" -eq 1 ] || [ "$reset_local" -eq 1 ]; }; then
              log_error "--reset-project/--reset-local require --vendor"
              exit 2
            fi

            mkdir -p "$target"

            if [ "$vendor" -eq 1 ]; then
              stage_dir="$(mktemp -d)"
              cleanup_stage() {
                rm -rf "$stage_dir"
              }
              trap cleanup_stage EXIT

              mkdir -p "$stage_dir/nixfied"
              cp -R "$source_root/." "$stage_dir/nixfied"
              if [ -f "$repo_root/README.md" ]; then
                cp "$repo_root/README.md" "$stage_dir/nixfied/README.md"
              fi
              chmod -R u+w "$stage_dir/nixfied" 2>/dev/null || true
              rm -rf "$stage_dir/nixfied/.git"
              rm -f "$stage_dir/nixfied/result"
              rm -f "$stage_dir/nixfied/.framework/.workspace"

              preserve_project=0
              preserve_local=0
              if [ -d "$target/nixfied/project" ] && [ "$reset_project" -eq 0 ]; then
                preserve_project=1
              fi
              if [ -d "$target/nixfied/local" ] && [ "$reset_local" -eq 0 ]; then
                preserve_local=1
              fi

              mkdir -p "$target/nixfied"
              chmod -R u+w "$target/nixfied" 2>/dev/null || true

              preserve_msg=""
              rsync_args=(-a --delete --chmod=Du+w,Fu+w)
              if [ "$preserve_project" -eq 1 ]; then
                rsync_args+=(--exclude='/project/')
                preserve_msg="nixfied/project/"
              fi
              if [ "$preserve_local" -eq 1 ]; then
                rsync_args+=(--exclude='/local/')
                if [ -n "$preserve_msg" ]; then
                  preserve_msg="$preserve_msg and nixfied/local/"
                else
                  preserve_msg="nixfied/local/"
                fi
              fi
              if [ -n "$preserve_msg" ]; then
                log_info "upgrading vendored wrapper (preserving $preserve_msg)"
              fi

              ${pkgs.rsync}/bin/rsync "''${rsync_args[@]}" "$stage_dir/nixfied/" "$target/nixfied/"
              rm -f "$target/nixfied/.framework/.workspace"

              cat > "$target/nixfied/VENDORED.txt" <<'NIXFIED_VENDORED'
      ${vendoredMetadata}
      NIXFIED_VENDORED

              cat > "$target/flake.nix" <<'NIXFIED_WRAPPER'
      ${vendoredWrapperFlake}
      NIXFIED_WRAPPER

              if [ "$upgrade" -eq 1 ] || [ -n "$preserve_msg" ]; then
                log_ok "vendored wrapper upgraded at $target/flake.nix"
              else
                log_ok "vendored wrapper flake generated at $target/flake.nix"
              fi
            else
              cat > "$target/flake.nix" <<'NIXFIED_WRAPPER'
      ${thinWrapperFlake}
      NIXFIED_WRAPPER
              if [ "$upgrade" -eq 1 ]; then
                log_ok "thin wrapper flake updated at $target/flake.nix"
              else
                log_ok "thin wrapper flake generated at $target/flake.nix"
              fi
            fi
    '';
in
{
  tasks = {
    framework-install = mkCommandTask {
      id = "task.framework.install";
      appName = "framework::install";
      kind = "utility";
      summary = "Install thin or vendored wrapper flake";
      description = "Creates a thin wrapper flake by default, or a vendored wrapper with --vendor. Re-running with --vendor preserves nixfied/project and nixfied/local by default.";
      runtimeInputs = frameworkInstallRuntimeInputs;
      usage = [
        "nix run .#framework::install"
        "nix run .#framework::install -- --vendor"
        "nix run .#framework::install -- --vendor --target ."
        "nix run .#framework::install -- --vendor --upgrade --target ."
      ];
      contractArgs = frameworkInstallContractArgs;
      command = mkFrameworkInstallCommand { };
      inherit ownerFile;
    };

    framework-upgrade = mkCommandTask {
      id = "task.framework.upgrade";
      appName = "framework::upgrade";
      kind = "utility";
      summary = "Upgrade vendored wrapper in-place";
      description = "Upgrades framework files while preserving nixfied/project and nixfied/local by default. Use --reset-project/--reset-local to overwrite those paths.";
      runtimeInputs = frameworkInstallRuntimeInputs;
      usage = [
        "nix run .#framework::upgrade -- --target ."
        "nix run .#framework::upgrade -- --target . --reset-project"
        "nix run .#framework::upgrade -- --target . --reset-local"
      ];
      contractArgs = frameworkUpgradeContractArgs;
      command = mkFrameworkInstallCommand {
        upgradeDefault = true;
      };
      inherit ownerFile;
    };
  };
}
