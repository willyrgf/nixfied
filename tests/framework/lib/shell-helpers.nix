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

  jsonRpcStub =
    let
      responder = pkgs.writeShellScript "nixfied-test-jsonrpc-responder" ''
        set -euo pipefail

        normalize_method_token() {
          printf '%s' "$1" | tr '[:lower:]' '[:upper:]' | tr -c 'A-Z0-9' '_'
        }

        read_request_body() {
          local line=""
          local content_length="0"
          local parsed_length=""

          while IFS= read -r line; do
            line="$(printf '%s' "$line" | ${pkgs.coreutils}/bin/tr -d '\r')"
            [ -n "$line" ] || break

            case "$line" in
              [Cc]ontent-[Ll]ength:*)
                parsed_length="$(
                  printf '%s' "$line" \
                    | ${pkgs.gnused}/bin/sed -nE 's/^[^:]+:[[:space:]]*([0-9]+).*$/\1/p'
                )"
                if [ -n "$parsed_length" ]; then
                  content_length="$parsed_length"
                fi
                ;;
            esac
          done

          if [ "$content_length" -gt 0 ] 2>/dev/null; then
            ${pkgs.coreutils}/bin/dd bs=1 count="$content_length" 2>/dev/null || true
          fi
        }

        result_json_for_method() {
          local method="$1"
          local token=""
          local file_var=""
          local json_var=""
          local result_file=""
          local result_json=""

          token="$(normalize_method_token "$method")"
          file_var="NIXFIED_JSONRPC_RESULT_FILE_$token"
          json_var="NIXFIED_JSONRPC_RESULT_JSON_$token"

          result_file="$(${pkgs.coreutils}/bin/printenv "$file_var" 2>/dev/null || true)"
          result_json="$(${pkgs.coreutils}/bin/printenv "$json_var" 2>/dev/null || true)"

          if [ -n "$result_file" ]; then
            ${pkgs.coreutils}/bin/cat "$result_file"
            return 0
          fi

          if [ -n "$result_json" ]; then
            printf '%s' "$result_json"
            return 0
          fi

          printf 'null'
        }

        request_body="$(read_request_body)"
        method="$(
          printf '%s' "$request_body" \
            | ${pkgs.gnused}/bin/sed -n 's/.*"method"[[:space:]]*:[[:space:]]*"\([^"]*\)".*/\1/p' \
            | ${pkgs.coreutils}/bin/head -n 1
        )"
        response_result="$(result_json_for_method "$method")"
        response_body="$(
          printf '{"jsonrpc":"2.0","id":1,"result":%s}' "$response_result"
        )"
        response_length="$(
          printf '%s' "$response_body" | ${pkgs.coreutils}/bin/wc -c | ${pkgs.coreutils}/bin/tr -d '[:space:]'
        )"

        printf 'HTTP/1.1 200 OK\r\n'
        printf 'Content-Type: application/json\r\n'
        printf 'Content-Length: %s\r\n' "$response_length"
        printf 'Connection: close\r\n'
        printf '\r\n'
        printf '%s' "$response_body"
      '';
    in
    {
      inherit responder;

      shellLib = ''
        ensure_jsonrpc_stub_responder() {
          if [ -n "''${NIXFIED_JSONRPC_RESPONDER_PATH:-}" ] && [ -x "$NIXFIED_JSONRPC_RESPONDER_PATH" ]; then
            return 0
          fi

          local responder_path="''${TMPDIR:-/tmp}/nixfied-jsonrpc-responder-$$.sh"
          ${pkgs.coreutils}/bin/cp ${responder} "$responder_path"
          ${pkgs.coreutils}/bin/chmod 0555 "$responder_path"
          export NIXFIED_JSONRPC_RESPONDER_PATH="$responder_path"
        }

        start_jsonrpc_stub() {
          local port="$1"
          ensure_jsonrpc_stub_responder
          ${pkgs.socat}/bin/socat "TCP-LISTEN:$port,bind=127.0.0.1,reuseaddr,fork" \
            "EXEC:$NIXFIED_JSONRPC_RESPONDER_PATH" \
            >/dev/null 2>&1 &
          bg_pids+=("$!")
        }
      '';
    };

  httpStub =
    let
      responder = pkgs.writeShellScript "nixfied-test-http-responder" ''
        set -euo pipefail

        normalize_path_token() {
          local path="$1"
          local token=""
          if [ "$path" = "/" ] || [ -z "$path" ]; then
            printf '%s' "ROOT"
            return 0
          fi
          token="$(printf '%s' "$path" | tr '[:lower:]' '[:upper:]' | tr -c 'A-Z0-9' '_')"
          while [ -n "$token" ] && [ "''${token#_}" != "$token" ]; do
            token="''${token#_}"
          done
          while [ -n "$token" ] && [ "''${token%_}" != "$token" ]; do
            token="''${token%_}"
          done
          printf '%s' "''${token:-ROOT}"
        }

        read_request() {
          local request_line=""
          local line=""

          IFS= read -r request_line || true
          request_line="$(printf '%s' "$request_line" | ${pkgs.coreutils}/bin/tr -d '\r')"

          HTTP_STUB_METHOD="$(${pkgs.gawk}/bin/awk '{ print $1 }' <<<"$request_line")"
          HTTP_STUB_PATH="$(${pkgs.gawk}/bin/awk '{ print $2 }' <<<"$request_line")"

          while IFS= read -r line; do
            line="$(printf '%s' "$line" | ${pkgs.coreutils}/bin/tr -d '\r')"
            [ -n "$line" ] || break
          done
        }

        env_value() {
          local name="$1"
          ${pkgs.coreutils}/bin/printenv "$name" 2>/dev/null || true
        }

        value_for_path() {
          local prefix="$1"
          local path="$2"
          local token=""
          local path_var=""
          local default_var=""
          local value=""

          token="$(normalize_path_token "$path")"
          path_var="NIXFIED_HTTP_''${prefix}_''${token}"
          default_var="NIXFIED_HTTP_''${prefix}_DEFAULT"

          value="$(env_value "$path_var")"
          if [ -n "$value" ]; then
            printf '%s' "$value"
            return 0
          fi

          value="$(env_value "$default_var")"
          if [ -n "$value" ]; then
            printf '%s' "$value"
            return 0
          fi

          case "$prefix" in
            STATUS)
              printf '%s' "200"
              ;;
            BODY)
              printf '%s' "ok"
              ;;
            CONTENT_TYPE)
              printf '%s' "text/plain"
              ;;
          esac
        }

        status_reason() {
          case "$1" in
            200) printf '%s' "OK" ;;
            404) printf '%s' "Not Found" ;;
            500) printf '%s' "Internal Server Error" ;;
            *) printf '%s' "OK" ;;
          esac
        }

        read_request

        response_status="$(value_for_path STATUS "$HTTP_STUB_PATH")"
        response_body="$(value_for_path BODY "$HTTP_STUB_PATH")"
        response_content_type="$(value_for_path CONTENT_TYPE "$HTTP_STUB_PATH")"
        response_length="$(
          printf '%s' "$response_body" | ${pkgs.coreutils}/bin/wc -c | ${pkgs.coreutils}/bin/tr -d '[:space:]'
        )"

        printf 'HTTP/1.1 %s %s\r\n' "$response_status" "$(status_reason "$response_status")"
        printf 'Content-Type: %s\r\n' "$response_content_type"
        printf 'Content-Length: %s\r\n' "$response_length"
        printf 'Connection: close\r\n'
        printf '\r\n'
        printf '%s' "$response_body"
      '';
    in
    {
      inherit responder;

      shellLib = ''
        start_http_stub() {
          local port="$1"
          ${pkgs.socat}/bin/socat "TCP-LISTEN:$port,bind=127.0.0.1,reuseaddr,fork" \
            "EXEC:${responder}" \
            >/dev/null 2>&1 &
          bg_pids+=("$!")
        }
      '';
    };
}
