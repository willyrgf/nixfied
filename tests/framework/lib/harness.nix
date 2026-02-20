{
  pkgs,
  model,
  registry,
  projectRoot ? ../../..,
}:
let
  executor = import ../../../nixfied/runner/executor.nix {
    inherit
      pkgs
      model
      registry
      projectRoot
      ;
  };

  orchestrator = import ../../../nixfied/runner/orchestrator.nix {
    inherit
      pkgs
      model
      registry
      projectRoot
      ;
  };
in
{
  inherit executor orchestrator;

  shellPrelude = ''
    fail() {
      echo "$1"
      exit 1
    }

    require_file() {
      local path="$1"
      if [ ! -f "$path" ]; then
        fail "missing file: $path"
      fi
    }

    require_contains() {
      local file="$1"
      local needle="$2"
      if ! ${pkgs.gnugrep}/bin/grep -Fq "$needle" "$file"; then
        echo "missing expected text '$needle' in $file"
        echo "--- $file"
        cat "$file"
        fail "assertion failed"
      fi
    }

    require_non_empty() {
      local value="$1"
      local label="$2"
      if [ -z "$value" ] || [ "$value" = "null" ]; then
        fail "missing value for $label"
      fi
    }

    extract_run_id() {
      local file="$1"
      ${pkgs.gnused}/bin/sed -n 's/^INFO: runId=\([^ ]*\).*/\1/p' "$file" | ${pkgs.coreutils}/bin/tail -n 1
    }
  '';
}
