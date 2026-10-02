# Adapter-owned protocol checks. Peer identity comes from HTTP; Reth owns RLPx.
{ pkgs }:
let
  rpcProbe = pkgs.writeScriptBin "nixfied-reth-probe" ''
    #!${pkgs.python3}/bin/python3
    ${builtins.readFile ./reth-probe.py}
  '';
in
pkgs.writeShellApplication {
  name = "nixfied-reth-probe";
  runtimeInputs = [ pkgs.curl pkgs.jq pkgs.reth ];
  text = ''
    if [[ "''${1:-}" != peer ]]; then
      exec ${rpcProbe}/bin/nixfied-reth-probe "$@"
    fi

    # Match the authored loopback domain and reject arguments before networking.
    if ! jq --exit-status --null-input --args '
      def port: test("^[0-9]{1,5}$") and (tonumber > 0 and tonumber <= 65535);
      def loopback:
        . == "::1" or
        (test("^127(\\.[0-9]{1,3}){3}$") and
          (split(".") | all(.[]; tonumber <= 255)));
      $ARGS.positional as $a |
      ($a | length) == 4 and ($a[1] | loopback) and
      ($a[2] | port) and ($a[3] | port)
    ' -- "$@" > /dev/null 2>&1; then
      echo "Reth endpoint probe failed" >&2
      exit 1
    fi

    host="$2"
    peer_port="$3"
    http_port="$4"
    if [[ "$host" == ::1 ]]; then
      host="[$host]"
    fi

    # Ignore the advertised address. Bound HTTP input and require one successful
    # JSON-RPC envelope with a complete enode public key before invoking Reth.
    if ! identity="$(curl --silent --fail --noproxy '*' --globoff \
      --max-time 2 --max-filesize 65536 --write-out '\n%{http_code}' \
      --header 'Content-Type: application/json' \
      --data '{"jsonrpc":"2.0","id":1,"method":"admin_nodeInfo","params":[]}' \
      "http://$host:$http_port" 2>/dev/null |
      jq --exit-status --raw-output --slurp '
        if length == 2 and .[1] == 200 and
          (.[0] | type == "object" and .jsonrpc == "2.0" and .id == 1 and
            (has("error") | not) and (.result.enode | type == "string"))
        then .[0].result.enode | capture("^(?<identity>enode://[0-9a-fA-F]{128})@").identity
        else error("invalid node identity response") end
      ' 2>/dev/null)"; then
      echo "Reth endpoint probe failed" >&2
      exit 1
    fi

    # Reth validates the identity and performs ECIES authentication and Hello.
    # No remote bytes or native diagnostics escape this short-lived probe.
    if ! reth p2p rlpx ping "$identity@$host:$peer_port" \
      --quiet --log.file.max-files 0 > /dev/null 2>&1; then
      echo "Reth endpoint probe failed" >&2
      exit 1
    fi
  '';
}
