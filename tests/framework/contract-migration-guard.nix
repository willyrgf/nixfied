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
  runtimeSources = builtins.filter (path: lib.hasSuffix ".nix" (toString path)) (
    lib.filesystem.listFilesRecursive ../../nixfied/framework/runtime
  );
  deprecatedKernelMarkers = [
    "validate-json"
    "query-json"
    "json-length"
    "mkValidator.nix"
    "run-record field"
    "adapter decode jsonrpc-result"
    "adapter decode supervisor-process-list"
    "summary.fields"
    "summary.steps.tsv"
    "meta.fields"
  ];
  filesWithDeprecatedKernelMarkers = builtins.filter (
    path:
    let
      source = builtins.readFile path;
    in
    lib.any (marker: lib.hasInfix marker source) deprecatedKernelMarkers
  ) runtimeSources;
  deletedHelperPaths = [
    ../../nixfied/framework/core/machine-output-validate.py
    ../../nixfied/framework/core/introspection-query.py
    ../../nixfied/framework/contracts/mkValidator.nix
    ../../nixfied/framework/contracts/render-cue.nix
    ../../nixfied/framework/runtime/helpers/run-registry.nix
    ../../tests/framework/snapshots/contracts/example.cue
  ];
in
assert builtins.all (path: !builtins.pathExists path) deletedHelperPaths;
assert disallowedJqFiles == [ ];
assert disallowedPythonFiles == [ ];
assert mismatchedAllowlistedFiles == [ ];
assert filesWithDeprecatedKernelMarkers == [ ];
assert pkgs.lib.hasInfix "nixfied-kernel machine-output run" machineOutputSource;
assert !(pkgs.lib.hasInfix "validatorProgram" machineOutputSource);
pkgs.runCommand "contract-migration-guard" { } ''
  echo "OK: hardening guards enforce deleted validators/CUE/run-registry, kernel-owned machine output, zero Python helpers, and no deprecated kernel seams in framework runtime" > "$out"
''
