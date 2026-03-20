{
  pkgs,
  lib,
}:
{
  name ? "nix-checks",
  flakeRef ? "path:.",
  formatterPkg ? (if pkgs ? nixfmt then pkgs.nixfmt else pkgs.nixfmt-rfc-style),
  nilPkg ? (if pkgs ? nil then pkgs.nil else throw "pkgs.nil is required for nix-checks"),
}:
let
  plainShellLogging = import ./plain-shell-logging.nix;
  shellCommon = import ./shell-common.nix { inherit pkgs; };
in
pkgs.writeShellScriptBin name ''
    set -euo pipefail

    mode="quick"
    flake_ref=${lib.escapeShellArg flakeRef}

    ${plainShellLogging {
      includeWarn = false;
      errorToStderr = true;
    }}
    ${shellCommon}

    usage() {
      cat <<'EOF'
  Usage: nix-checks [--mode <quick|full>] [--quick] [--full] [--flake <ref>] [--help]

  Modes:
    quick  Run nixfmt --check, nil diagnostics, nix flake show, and nix run .#help.
    full   Run quick mode plus nix flake check.
  EOF
    }

    list_nix_files() {
      ${pkgs.findutils}/bin/find . -type f -name '*.nix' | ${pkgs.coreutils}/bin/sort
    }

    run_nixfmt_check() {
      local -a nix_files

      mapfile -t nix_files < <(list_nix_files)

      if [ "''${#nix_files[@]}" -eq 0 ]; then
        log_skip "no nix files found for formatting check"
        return 0
      fi

      log_info "checking nix formatting files=''${#nix_files[@]}"
      ${formatterPkg}/bin/nixfmt --check "''${nix_files[@]}"
      log_ok "nix formatting check passed files=''${#nix_files[@]}"
    }

    run_nil_diagnostics_check() {
      local -a nix_files
      local -a failed_files
      local nix_file

      mapfile -t nix_files < <(list_nix_files)

      if [ "''${#nix_files[@]}" -eq 0 ]; then
        log_skip "no nix files found for nil diagnostics"
        return 0
      fi

      log_info "checking nil diagnostics files=''${#nix_files[@]}"
      if ${nilPkg}/bin/nil diagnostics "''${nix_files[@]}" > /dev/null 2>&1; then
        log_ok "nil diagnostics check passed files=''${#nix_files[@]}"
        return 0
      fi

      failed_files=()
      for nix_file in "''${nix_files[@]}"; do
        if ! ${nilPkg}/bin/nil diagnostics "$nix_file" > /dev/null 2>&1; then
          failed_files+=("$nix_file")
        fi
      done

      if [ "''${#failed_files[@]}" -eq 0 ]; then
        log_error "nil diagnostics check failed files=unknown"
        return 1
      fi

      log_error "nil diagnostics check failed files=''${#failed_files[@]}"
      for nix_file in "''${failed_files[@]}"; do
        log_error "nil diagnostics failed file=$nix_file"
      done
      return 1
    }

    run_flake_show_check() {
      log_info "checking flake output surface ref=$flake_ref"
      ${pkgs.nix}/bin/nix flake show --no-write-lock-file "$flake_ref" > /dev/null
      log_ok "flake output surface check passed ref=$flake_ref"
    }

    run_help_check() {
      local help_ref
      help_ref="$flake_ref#help"
      log_info "checking help surface ref=$help_ref"
      ${pkgs.nix}/bin/nix run "$help_ref" > /dev/null
      log_ok "help surface check passed ref=$help_ref"
    }

    run_flake_check() {
      log_info "checking flake checks ref=$flake_ref"
      ${pkgs.nix}/bin/nix flake check -L --no-write-lock-file "$flake_ref"
      log_ok "flake checks passed ref=$flake_ref"
    }

    should_skip_flake_check() {
      [ -n "''${NIX_BUILD_TOP:-}" ] || [ -n "''${NIXFIED_PARENT_WORKFLOW_ID:-}" ]
    }

    while [ "$#" -gt 0 ]; do
      case "$1" in
        --mode)
          mode="$(nixfied_require_next_arg_with_usage usage --mode "a value" "$@")"
          shift 2
          ;;
        --quick)
          mode="quick"
          shift
          ;;
        --full)
          mode="full"
          shift
          ;;
        --flake)
          flake_ref="$(nixfied_require_next_arg_with_usage usage --flake "a value" "$@")"
          shift 2
          ;;
        --help)
          usage
          exit 0
          ;;
        *)
          nixfied_unknown_arg_with_usage usage "$1"
          ;;
      esac
    done

    case "$mode" in
      quick|full)
        ;;
      *)
        nixfied_exit_usage_with_usage usage "unsupported mode: $mode"
        ;;
    esac

    log_info "running nix checks mode=$mode flake=$flake_ref"
    run_nixfmt_check
    run_nil_diagnostics_check
    run_flake_show_check
    run_help_check

    if [ "$mode" = "full" ]; then
      if should_skip_flake_check; then
        log_skip "flake checks skipped inside nix build sandbox or parent workflow ref=$flake_ref"
      else
        run_flake_check
      fi
    fi

    log_ok "nix checks passed mode=$mode flake=$flake_ref"
''
