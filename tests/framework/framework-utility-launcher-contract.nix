{ pkgs, apps }:
pkgs.runCommand "framework-utility-launcher-contract" { } ''
  set -euo pipefail

  for app in ${apps."framework::install".program} ${apps."framework::upgrade".program}; do
    if ${pkgs.gnugrep}/bin/grep -Fq 'run-runtime-app.nix' "$app"; then
      echo "framework utility launcher must not use run-runtime-app.nix"
      exit 1
    fi

    if ${pkgs.gnugrep}/bin/grep -Fq 'run-selected-app.nix' "$app"; then
      echo "framework utility launcher must not use run-selected-app.nix"
      exit 1
    fi

    if ${pkgs.gnugrep}/bin/grep -Fq '#run-task' "$app"; then
      echo "framework utility launcher must not proxy through public #run-task"
      exit 1
    fi
  done

  echo "OK: framework utility apps use direct launchers" > "$out"
''
