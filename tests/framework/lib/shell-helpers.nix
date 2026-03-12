{ pkgs }:
let
  plainShellLogging = import ../../../nixfied/lib/plain-shell-logging.nix;

  mkShellPrelude =
    {
      stderr ? false,
    }:
    let
      redirect = if stderr then " >&2" else "";
    in
    ''
      fail() {
        echo "$1"${redirect}
        exit 1
      }

      require_file() {
        local path="$1"
        if [ ! -f "$path" ]; then
          fail "missing file: $path"
        fi
      }

      require_not_file() {
        local path="$1"
        if [ -f "$path" ]; then
          fail "unexpected file: $path"
        fi
      }

      require_contains() {
        local file="$1"
        local needle="$2"
        if ! ${pkgs.gnugrep}/bin/grep -Fq -- "$needle" "$file"; then
          echo "--- $file"${redirect}
          cat "$file"${redirect}
          fail "expected '$needle' in $file"
        fi
      }

      require_not_contains() {
        local file="$1"
        local needle="$2"
        if ${pkgs.gnugrep}/bin/grep -Fq -- "$needle" "$file"; then
          echo "--- $file"${redirect}
          cat "$file"${redirect}
          fail "unexpected '$needle' in $file"
        fi
      }

      require_non_empty() {
        local value="$1"
        local label="$2"
        if [ -z "$value" ] || [ "$value" = "null" ]; then
          fail "missing value for $label"
        fi
      }

      read_trimmed_file() {
        local path="$1"
        require_file "$path"
        ${pkgs.coreutils}/bin/tr -d '\n' < "$path"
      }
    '';
in
{
  loggingPrelude = plainShellLogging { };

  loggingPreludeWithStop = plainShellLogging {
    includeStop = true;
    stopFormat = "INFO: stopping %s\n";
  };

  errorLoggingPrelude = plainShellLogging {
    includeInfo = false;
    includeWarn = false;
    includeOk = false;
    includeSkip = false;
  };

  shellPrelude = mkShellPrelude { };
  stderrShellPrelude = mkShellPrelude { stderr = true; };
}
