# Adapter-owned protocol checks. Clients own transport, framing and JWT signing;
# the runtime owns listener witnesses, attempt deadlines and whole-round retries.
{ pkgs }:
pkgs.writeShellApplication {
  name = "nixfied-reth-probe";
  runtimeInputs = [ pkgs.curl pkgs.jq pkgs.reth pkgs.websocat pkgs.jwt-cli pkgs.xxd pkgs.coreutils ];
  text = ''
    fail() {
      echo "Reth endpoint probe failed" >&2
      exit 1
    }

    # Reject malformed modes and endpoint arguments before any networking.
    if ! jq --exit-status --null-input --args '
      def port: test("^[0-9]{1,5}$") and (tonumber > 0 and tonumber <= 65535);
      def loopback:
        . == "::1" or
        (test("^127(\\.[0-9]{1,3}){3}$") and
          (split(".") | all(.[]; tonumber <= 255)));
      $ARGS.positional as $a |
      ($a[0] == "http" or $a[0] == "ws" or $a[0] == "authrpc" or $a[0] == "peer") and
      ($a | length) == (if $a[0] == "authrpc" or $a[0] == "peer" then 4 else 3 end) and
      ($a[1] | loopback) and ($a[2] | port) and
      (if $a[0] == "peer" then $a[3] | port else true end)
    ' -- "$@" > /dev/null 2>&1; then
      fail
    fi

    mode="$1"
    host="$2"
    port="$3"
    if [[ "$host" == ::1 ]]; then
      host="[$host]"
    fi

    # Exactly one response, plus curl's independent HTTP status where applicable.
    # Return only the public peer identity; other modes produce no probe output.
    check_response() {
      jq --exit-status --raw-output --slurp --arg mode "$mode" '
        def envelope:
          type == "object" and .jsonrpc == "2.0" and .id == 1 and
          (has("error") | not) and has("result");
        if length == (if $mode == "ws" then 1 else 2 end) and
          (if $mode == "ws" then true else .[1] == 200 end) and
          (.[0] | envelope)
        then .[0].result |
          if $mode == "peer" then
            .enode | capture("^(?<identity>enode://[0-9a-fA-F]{128})@").identity
          elif $mode == "authrpc" then
            type == "array" and length > 0 and
              all(.[]; type == "string" and test("^engine_[A-Za-z0-9]+$"))
          else type == "string" and test("^0x(?:0|[1-9a-fA-F][0-9a-fA-F]*)$") end
        else error("invalid RPC response") end
      '
    }

    request='{"jsonrpc":"2.0","id":1,"method":"eth_blockNumber","params":[]}'
    if [[ "$mode" == ws ]]; then
      # One text request and response. Prefix binary messages so they cannot be
      # mistaken for JSON. Bound both individual frames and reassembled messages.
      if ! { printf '%s\n' "$request" |
        websocat -q -t -n -1 --max-ws-frame-length 65536 \
          --max-ws-message-length 65536 --buffer-size 65536 \
          --binary-prefix invalid-binary: "ws://$host:$port" |
        check_response; } > /dev/null 2>&1; then
        fail
      fi
      exit 0
    fi

    token=""
    if [[ "$mode" == authrpc ]]; then
      # Encode bounded file bytes before shell capture, which discards NUL.
      # The hex view must describe exactly 64 ASCII hex digits and an optional
      # newline. Validate before decoding: xxd tolerates other malformed input.
      if ! encoded_secret="$(head -c 67 "$4/reth/config/jwt.hex" 2>/dev/null |
        xxd -p -c 67 2>/dev/null)" ||
        [[ ! "$encoded_secret" =~ ^(3[0-9]|[46][1-6]){64}(0a)?$ ]]; then
        fail
      fi
      # Decode the validated file view and then its hex key. The signer reads
      # raw key bytes through a descriptor, never a secret argv or temp file.
      if ! token="$(jwt encode --alg HS256 \
        --secret @<(printf '%s' "$encoded_secret" | xxd -r -p | xxd -r -p) '{}' 2>/dev/null)"; then
        fail
      fi
      request='{"jsonrpc":"2.0","id":1,"method":"engine_exchangeCapabilities","params":[[]]}'
    elif [[ "$mode" == peer ]]; then
      peer_port="$port"
      port="$4"
      request='{"jsonrpc":"2.0","id":1,"method":"admin_nodeInfo","params":[]}'
    fi

    # Ignore curl configuration and proxies, do not follow redirects, and bound
    # response bytes. Token-bearing headers travel on stdin, not through argv.
    if ! result="$(
      { if [[ -n "$token" ]]; then printf 'Authorization: Bearer %s\n' "$token"; fi; } |
        curl -q --silent --fail --noproxy '*' --globoff \
          --max-time 2 --max-filesize 65536 --write-out '\n%{http_code}' \
          --header 'Content-Type: application/json' --header @- --data "$request" \
          "http://$host:$port" 2>/dev/null |
        check_response 2>/dev/null
    )"; then
      fail
    fi

    if [[ "$mode" == peer ]]; then
      # Ignore the advertised address. Reth owns ECIES authentication and Hello.
      if ! reth p2p rlpx ping "$result@$host:$peer_port" \
        --quiet --log.file.max-files 0 > /dev/null 2>&1; then
        fail
      fi
    fi
  '';
}
