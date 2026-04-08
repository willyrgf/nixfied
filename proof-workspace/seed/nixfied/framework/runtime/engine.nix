{
  pkgs,
  orchestratorProgram,
  serviceDispatcherProgram,
}:
pkgs.writeShellScriptBin "nixfied-runtime" ''
    set -euo pipefail

    usage() {
      cat <<'EOF'
  usage: nixfied-runtime <run-task|run-workflow|run-workflow-parallel|runs|stop-run|stop-all-runs|run-service|has-service-op> ...
  EOF
    }

    if [ "$#" -lt 1 ]; then
      usage >&2
      exit 2
    fi

    subcommand="$1"
    shift

    case "$subcommand" in
      run-task|run-workflow|run-workflow-parallel|runs|stop-run|stop-all-runs)
        exec ${pkgs.lib.escapeShellArg orchestratorProgram} "$subcommand" "$@"
        ;;
      run-service|has-service-op)
        exec ${pkgs.lib.escapeShellArg serviceDispatcherProgram} "$subcommand" "$@"
        ;;
      *)
        echo "ERROR: unknown subcommand '$subcommand'" >&2
        usage >&2
        exit 2
        ;;
    esac
''
