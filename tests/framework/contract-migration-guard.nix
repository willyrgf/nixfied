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
  pythonMarkerLines =
    source:
    builtins.filter (
      line:
      lib.any (marker: lib.hasInfix marker line) [
        "pkgs.python3"
        "/bin/python3"
        "python3"
      ]
    ) (lib.splitString "\n" source);
  jqAllowlist = [ ];
  allowlistedPathStrings = map (entry: toString entry.path) jqAllowlist;
  disallowedJqFiles = builtins.filter (
    path:
    let
      pathString = toString path;
      jqLines = jqMarkerLines (builtins.readFile path);
    in
    jqLines != [ ] && !(builtins.elem pathString allowlistedPathStrings)
  ) frameworkSources;
  disallowedPythonFiles = builtins.filter (
    path:
    let
      pythonLines = pythonMarkerLines (builtins.readFile path);
    in
    pythonLines != [ ]
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
assert disallowedPythonFiles == [ ];
assert mismatchedAllowlistedFiles == [ ];
assert pkgs.lib.hasInfix "NIXFIED_MACHINE_OUTPUT_FILE=\"$payload_file\"" machineOutputSource;
assert pkgs.lib.hasInfix "did not write machine payload to declared file" machineOutputSource;
assert (!pkgs.lib.hasInfix "\"$target_stdout\" >\"$payload_file\"" machineOutputSource);
assert (!pkgs.lib.hasInfix "\"$target_stdout\" >>\"$payload_file\"" machineOutputSource);
assert (!pkgs.lib.hasInfix "<\"$target_stdout\"" machineOutputSource);
assert (!pkgs.lib.hasInfix "stdout-filter" machineOutputSource);
pkgs.runCommand "contract-migration-guard" { } ''
  echo "OK: contract migration guards enforce deleted helpers, explicit machine channels, zero Python helpers, and no framework jq exception sites" > "$out"
''
