{
  pkgs,
  lib,
}:
{
  name ? "nix-checks",
  flakeRef ? ".",
  formatterPkg ? (if pkgs ? nixfmt then pkgs.nixfmt else pkgs.nixfmt-rfc-style),
  deadnixPkg ? (
    if pkgs ? deadnix then pkgs.deadnix else throw "pkgs.deadnix is required for nix-checks"
  ),
  statixPkg ? (if pkgs ? statix then pkgs.statix else throw "pkgs.statix is required for nix-checks"),
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
    printf '%s\n' \
      "Usage: nix-checks [--mode <quick|full>] [--quick] [--full] [--flake <ref>] [--help]" \
      "" \
      "Modes:" \
      "  quick  Run nixfmt --check, deadnix, statix, nil diagnostics, nix flake show, and nix run .#help." \
      "  full   Run quick mode plus nix flake check."
  }

  read_nix_files() {
    local target_name="$1"
    local -n target_ref="$target_name"
    local nix_file

    target_ref=()
    while IFS= read -r nix_file; do
      target_ref+=("$nix_file")
    done < <(${pkgs.findutils}/bin/find . -type f -name '*.nix' | ${pkgs.coreutils}/bin/sort)
  }

  run_and_capture_output() {
    local target_name="$1"
    local -n target_ref="$target_name"
    local output_file
    local status

    shift || true

    target_ref=""
    output_file="$(${pkgs.coreutils}/bin/mktemp "''${TMPDIR:-/tmp}/nix-checks-output.XXXXXX")"

    set +e
    "$@" 2>&1 | ${pkgs.coreutils}/bin/tee "$output_file"
    status=$?
    set -e

    if [ -s "$output_file" ]; then
      target_ref="$(${pkgs.coreutils}/bin/cat "$output_file")"
    fi

    ${pkgs.coreutils}/bin/rm -f "$output_file"
    return "$status"
  }

  run_nixfmt_check() {
    local -a nix_files

    read_nix_files nix_files

    if [ "''${#nix_files[@]}" -eq 0 ]; then
      log_skip "no nix files found for formatting check"
      return 0
    fi

    log_info "checking nix formatting files=''${#nix_files[@]}"
    ${formatterPkg}/bin/nixfmt --check "''${nix_files[@]}"
    log_ok "nix formatting check passed files=''${#nix_files[@]}"
  }

  run_deadnix_check() {
    local -a nix_files
    local -a issue_files
    local nix_file
    local aggregate_output
    local file_output
    local aggregate_status
    local file_status

    read_nix_files nix_files

    if [ "''${#nix_files[@]}" -eq 0 ]; then
      log_skip "no nix files found for deadnix"
      return 0
    fi

    log_info "checking dead code files=''${#nix_files[@]}"
    if run_and_capture_output aggregate_output ${deadnixPkg}/bin/deadnix "''${nix_files[@]}"; then
      aggregate_status=0
    else
      aggregate_status=$?
    fi

    if [ "$aggregate_status" -eq 0 ] && [ -z "$aggregate_output" ]; then
      log_ok "dead code check passed files=''${#nix_files[@]}"
      return 0
    fi

    issue_files=()
    for nix_file in "''${nix_files[@]}"; do
      if file_output="$(${deadnixPkg}/bin/deadnix "$nix_file" 2>&1)"; then
        file_status=0
      else
        file_status=$?
      fi

      if [ "$file_status" -ne 0 ] || [ -n "$file_output" ]; then
        issue_files+=("$nix_file")
      fi
    done

      if [ "''${#issue_files[@]}" -eq 0 ]; then
        log_error "deadnix reported issues files=unknown"
        return 1
      fi

      log_error "deadnix reported issues files=''${#issue_files[@]}"
      for nix_file in "''${issue_files[@]}"; do
        log_error "deadnix reported file=$nix_file"
      done
      return 1
    }

  run_statix_check() {
    local -a nix_files
    local -a issue_files
    local nix_file
    local aggregate_output
    local file_output
    local aggregate_status
    local file_status

    read_nix_files nix_files

    if [ "''${#nix_files[@]}" -eq 0 ]; then
      log_skip "no nix files found for statix"
      return 0
    fi

    log_info "checking statix lints files=''${#nix_files[@]}"
    if run_and_capture_output aggregate_output ${statixPkg}/bin/statix check .; then
      aggregate_status=0
    else
      aggregate_status=$?
    fi

    if [ "$aggregate_status" -eq 0 ] && [ -z "$aggregate_output" ]; then
      log_ok "statix lint check passed files=''${#nix_files[@]}"
      return 0
    fi

    issue_files=()
    for nix_file in "''${nix_files[@]}"; do
      if file_output="$(${statixPkg}/bin/statix check "$nix_file" 2>&1)"; then
        file_status=0
      else
        file_status=$?
      fi

      if [ "$file_status" -ne 0 ] || [ -n "$file_output" ]; then
        issue_files+=("$nix_file")
      fi
    done

      if [ "''${#issue_files[@]}" -eq 0 ]; then
        log_error "statix reported issues files=unknown"
        return 1
      fi

      log_error "statix reported issues files=''${#issue_files[@]}"
      for nix_file in "''${issue_files[@]}"; do
        log_error "statix reported file=$nix_file"
      done
      return 1
    }

  run_nil_diagnostics_check() {
    local -a nix_files
    local -a issue_files
    local nix_file
    local aggregate_output
    local file_output
    local aggregate_status
    local file_status

    read_nix_files nix_files

    if [ "''${#nix_files[@]}" -eq 0 ]; then
      log_skip "no nix files found for nil diagnostics"
      return 0
    fi

    log_info "checking nil diagnostics files=''${#nix_files[@]}"
    if run_and_capture_output aggregate_output ${nilPkg}/bin/nil diagnostics "''${nix_files[@]}"; then
      aggregate_status=0
    else
      aggregate_status=$?
    fi

    if [ "$aggregate_status" -eq 0 ] && [ -z "$aggregate_output" ]; then
      log_ok "nil diagnostics check passed files=''${#nix_files[@]}"
      return 0
    fi

    issue_files=()
    for nix_file in "''${nix_files[@]}"; do
      if file_output="$(${nilPkg}/bin/nil diagnostics "$nix_file" 2>&1)"; then
        file_status=0
      else
        file_status=$?
      fi

      if [ "$file_status" -ne 0 ] || [ -n "$file_output" ]; then
        issue_files+=("$nix_file")
      fi
    done

    if [ "''${#issue_files[@]}" -eq 0 ]; then
      log_error "nil diagnostics reported issues files=unknown"
      return 1
    fi

    log_error "nil diagnostics reported issues files=''${#issue_files[@]}"
    for nix_file in "''${issue_files[@]}"; do
      log_error "nil diagnostics reported file=$nix_file"
    done
    return 1
  }

  run_flake_show_check() {
    log_info "checking flake output surface ref=$flake_ref"
    ${pkgs.nix}/bin/nix flake show --no-write-lock-file "$flake_ref"
    log_ok "flake output surface check passed ref=$flake_ref"
  }

  run_help_check() {
    local help_ref
    help_ref="$flake_ref#help"
    log_info "checking help surface ref=$help_ref"
    ${pkgs.nix}/bin/nix run "$help_ref"
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

  quick_checks_failed=0

  log_info "running nix checks mode=$mode flake=$flake_ref"
  if ! run_nixfmt_check; then
    quick_checks_failed=1
  fi
  if ! run_deadnix_check; then
    quick_checks_failed=1
  fi
  if ! run_statix_check; then
    quick_checks_failed=1
  fi
  if ! run_nil_diagnostics_check; then
    quick_checks_failed=1
  fi

  if [ "$quick_checks_failed" -ne 0 ]; then
    nixfied_exit_generic "nix checks failed mode=$mode flake=$flake_ref"
  fi

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
