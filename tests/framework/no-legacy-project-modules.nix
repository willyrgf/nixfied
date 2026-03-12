{ pkgs }:
pkgs.runCommand "no-legacy-project-modules" { } ''
  matches_file="$TMPDIR/project-modules-matches.txt"
  ${pkgs.ripgrep}/bin/rg -n "project\\.modules\\." \
    ${../../nixfied/framework} \
    ${../../nixfied/framework/runtime} \
    ${../../nixfied/runner} \
    ${../../nixfied/registry} \
    -g '*.nix' > "$matches_file" || true

  if [ -s "$matches_file" ]; then
    cat "$matches_file" >&2
    echo "ERROR: legacy project.modules reads remain in runtime/framework code" >&2
    exit 1
  fi

  echo "OK: no legacy project.modules reads remain in runtime/framework code" > "$out"
''
