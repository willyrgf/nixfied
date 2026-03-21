{ pkgs, apps }:
pkgs.runCommand "runtime-control-launcher-contract" { } ''
  set -euo pipefail

  for app in runs stop-run stop-all-runs; do
    case "$app" in
      runs)
        program=${apps.runs.program}
        ;;
      stop-run)
        program=${apps."stop-run".program}
        ;;
      stop-all-runs)
        program=${apps."stop-all-runs".program}
        ;;
    esac

    if ${pkgs.gnugrep}/bin/grep -Fq 'run-runtime-app.nix' "$program"; then
      echo "runtime control app=$app must not use run-runtime-app.nix"
      exit 1
    fi

    if ${pkgs.gnugrep}/bin/grep -Fq 'run-selected-app.nix' "$program"; then
      echo "runtime control app=$app must not use run-selected-app.nix"
      exit 1
    fi

    if ! ${pkgs.gnugrep}/bin/grep -Fq 'nixfied-orchestrator-control' "$program"; then
      echo "runtime control app=$app must target nixfied-orchestrator-control"
      exit 1
    fi
  done

  echo "OK: runtime control launchers are direct" > "$out"
''
