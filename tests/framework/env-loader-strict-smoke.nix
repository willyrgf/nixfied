{ pkgs }:
let
  shellHelpers = import ./lib/shell-helpers.nix { inherit pkgs; };
  loggingPrelude = shellHelpers.errorLoggingPrelude;

  envLoader = import ../../nixfied/.framework/lib/env-loader.nix {
    inherit
      pkgs
      loggingPrelude
      ;
    project = {
      tooling = {
        envFile = {
          enable = true;
          strict = true;
          allow = [
            {
              name = "REQ";
              type = "string";
              required = true;
            }
            {
              name = "KNOWN";
              type = "string";
            }
            {
              name = "OPT_INT";
              type = "int";
              default = 12;
            }
          ];
        };
      };
    };
  };

  sourceWrapper = pkgs.writeShellScript "env-loader-source-wrapper" ''
    set -euo pipefail
    source ${envLoader.loadEnvFile} "$1"
    printf 'REQ=%s\n' "$REQ"
    printf 'KNOWN=%s\n' "$KNOWN"
    printf 'OPT_INT=%s\n' "$OPT_INT"
  '';
in
pkgs.runCommand "env-loader-strict-smoke" { } ''
    set -euo pipefail

    workdir="$TMPDIR/env-loader"
    mkdir -p "$workdir"

    set +e
    "${envLoader.loadEnvFile}" "$workdir/missing.env" > "$TMPDIR/missing.out" 2>&1
    missing_rc="$?"
    set -e
    if [ "$missing_rc" -eq 0 ]; then
      echo "expected missing strict .env to fail for required key"
      cat "$TMPDIR/missing.out"
      exit 1
    fi
    ${pkgs.gnugrep}/bin/grep -Fq "ERROR: required env key missing key=REQ source=.env" "$TMPDIR/missing.out"

    cat > "$workdir/unknown.env" <<'EOF'
  REQ=file-required
  KNOWN=file-known
  EXTRA=unexpected
  EOF

    set +e
    "${envLoader.loadEnvFile}" "$workdir/unknown.env" > "$TMPDIR/unknown.out" 2>&1
    unknown_rc="$?"
    set -e
    if [ "$unknown_rc" -eq 0 ]; then
      echo "expected strict .env with unknown key to fail"
      cat "$TMPDIR/unknown.out"
      exit 1
    fi
    ${pkgs.gnugrep}/bin/grep -Fq "ERROR: unknown .env key key=EXTRA (strict mode enabled)" "$TMPDIR/unknown.out"

    cat > "$workdir/good.env" <<'EOF'
  KNOWN=file-known
  EOF

    REQ=seed KNOWN=host-value "${sourceWrapper}" "$workdir/good.env" > "$TMPDIR/good.out" 2>&1
    ${pkgs.gnugrep}/bin/grep -Fq "REQ=seed" "$TMPDIR/good.out"
    ${pkgs.gnugrep}/bin/grep -Fq "KNOWN=host-value" "$TMPDIR/good.out"
    ${pkgs.gnugrep}/bin/grep -Fq "OPT_INT=12" "$TMPDIR/good.out"

    echo "OK: env loader strict-mode contract validated" > "$out"
''
