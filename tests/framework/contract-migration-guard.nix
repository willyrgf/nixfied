{ pkgs }:
let
  lib = pkgs.lib;
  frameworkSources = builtins.filter (path: lib.hasSuffix ".nix" (toString path)) (
    lib.filesystem.listFilesRecursive ../../nixfied/framework
  );
  jqMarkerLines =
    source:
    builtins.filter (
      line:
      lib.any (marker: lib.hasInfix marker line) [
        "\${pkgs.jq}/bin/jq"
        "/bin/jq"
        "pkgs.jq"
        "jq = pkgs.jq"
      ]
    ) (lib.splitString "\n" source);
  jqAllowlist = [
    {
      path = ../../nixfied/framework/runtime/helpers/env-loader.nix;
      count = 1;
      snippets = [
        "printf '%s' \"$value\" |"
        "/bin/jq -e . >/dev/null 2>&1"
      ];
    }
    {
      path = ../../nixfied/framework/runtime/helpers/probe-commands.nix;
      count = 1;
      snippets = [
        ") |"
        "/bin/jq -"
        "'\${fieldExpr}'"
      ];
    }
    {
      path = ../../nixfied/framework/runtime/helpers/probe-plan-runtime.nix;
      count = 1;
      snippets = [
        "helios_block_number="
        "/bin/jq -r '.result // empty'"
      ];
    }
    {
      path = ../../nixfied/framework/runtime/helpers/shell-contract.nix;
      count = 1;
      snippets = [
        "NIXFIED_CONTRACT_JQ="
        "/bin/jq"
      ];
    }
    {
      path = ../../nixfied/framework/runtime/services/helios/lifecycle.nix;
      count = 2;
      snippets = [
        "slot=\"$(echo \"$FINALIZED_JSON\" |"
        "/bin/jq -r '.data.header.message.slot|tonumber'"
        "checkpoint=\"$(echo \"$EPOCH_JSON\" |"
        "/bin/jq -r '.data.root // empty'"
      ];
    }
    {
      path = ../../nixfied/framework/runtime/services/supervisor/status.nix;
      count = 3;
      snippets = [
        "  jq = pkgs.jq;"
        "TOTAL=$("
        "/bin/jq -r 'length' <<<\"$SUPERVISOR_PROCESS_JSON\")"
        "/bin/jq -r '"
      ];
    }
  ];
  allowlistedPathStrings = map (entry: toString entry.path) jqAllowlist;
  disallowedJqFiles = builtins.filter (
    path:
    let
      pathString = toString path;
      jqLines = jqMarkerLines (builtins.readFile path);
    in
    jqLines != [ ] && !(builtins.elem pathString allowlistedPathStrings)
  ) frameworkSources;
  mismatchedAllowlistedFiles = builtins.filter (
    entry:
    let
      source = builtins.readFile entry.path;
      jqLines = jqMarkerLines (builtins.readFile entry.path);
    in
    (builtins.length jqLines != entry.count)
    || !(builtins.all (snippet: lib.hasInfix snippet source) entry.snippets)
  ) jqAllowlist;
  machineOutputSource = builtins.readFile ../../nixfied/framework/core/mkMachineOutputPrograms.nix;
  deletedHelperPaths = [
    ../../nixfied/framework/core/machine-output-validate.py
    ../../nixfied/framework/core/introspection-query.py
  ];
in
assert builtins.all (path: !builtins.pathExists path) deletedHelperPaths;
assert disallowedJqFiles == [ ];
assert mismatchedAllowlistedFiles == [ ];
assert pkgs.lib.hasInfix "NIXFIED_MACHINE_OUTPUT_FILE=\"$payload_file\"" machineOutputSource;
assert pkgs.lib.hasInfix "did not write machine payload to declared file" machineOutputSource;
assert (!pkgs.lib.hasInfix "\"$target_stdout\" >\"$payload_file\"" machineOutputSource);
assert (!pkgs.lib.hasInfix "\"$target_stdout\" >>\"$payload_file\"" machineOutputSource);
assert (!pkgs.lib.hasInfix "<\"$target_stdout\"" machineOutputSource);
assert (!pkgs.lib.hasInfix "stdout-filter" machineOutputSource);
pkgs.runCommand "contract-migration-guard" { } ''
  echo "OK: contract migration guards enforce deleted helpers, explicit machine channels, and exact framework jq exception sites" > "$out"
''
