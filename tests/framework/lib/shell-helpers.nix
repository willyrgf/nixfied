{ pkgs }:
let
  plainShellLogging = import ../../../nixfied/framework/core/plain-shell-logging.nix;
  shellCommon = import ../../../nixfied/framework/core/shell-common.nix { inherit pkgs; };
  postgresPkg = if pkgs ? postgresql_16 then pkgs.postgresql_16 else pkgs.postgresql;

  mkShellPrelude =
    {
      stderr ? false,
    }:
    let
      redirect = if stderr then " >&2" else "";
    in
    ''
      ${shellCommon}

      fail() {
        echo "$1"${redirect}
        exit "$NIXFIED_EXIT_GENERIC"
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

  postgresCapabilityPrelude = ''
    postgres_bootstrap_probe() {
      local probe_root
      local probe_pgdata
      local probe_log

      probe_root="$(${pkgs.coreutils}/bin/mktemp -d "''${TMPDIR:-/tmp}/nixfied-postgres-probe.XXXXXX")"
      probe_pgdata="$probe_root/data"
      probe_log="$probe_root/initdb.log"
      mkdir -p "$probe_pgdata"

      if ${postgresPkg}/bin/initdb -D "$probe_pgdata" --auth=trust --username=postgres --no-locale >"$probe_log" 2>&1; then
        ${postgresPkg}/bin/pg_ctl -D "$probe_pgdata" -m immediate stop >/dev/null 2>&1 || true
        rm -rf "$probe_root"
        return 0
      fi

      if ${pkgs.gnugrep}/bin/grep -Fq "could not create shared memory segment" "$probe_log" \
        && ${pkgs.gnugrep}/bin/grep -Fq "Failed system call was shmget" "$probe_log"; then
        rm -rf "$probe_root"
        return 1
      fi

      echo "--- $probe_log" >&2
      cat "$probe_log" >&2
      rm -rf "$probe_root"
      echo "unexpected postgres bootstrap probe failure" >&2
      exit 1
    }

    skip_if_postgres_bootstrap_unavailable() {
      local label="$1"
      if postgres_bootstrap_probe; then
        return 0
      fi

      printf 'OK: SKIP: %s requires PostgreSQL bootstrap support in the build sandbox\n' "$label" > "$out"
      exit 0
    }
  '';
}
