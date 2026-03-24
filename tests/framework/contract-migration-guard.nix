{ pkgs }:
let
  lib = pkgs.lib;
  guardPath = ../../tests/framework/contract-migration-guard.nix;
  frameworkSources = builtins.filter (path: lib.hasSuffix ".nix" (toString path)) (
    lib.filesystem.listFilesRecursive ../../nixfied/framework
  );
  authoringSources =
    builtins.filter (path: path != guardPath && lib.hasSuffix ".nix" (toString path))
      (
        (lib.filesystem.listFilesRecursive ../../nixfied/modules)
        ++ (lib.filesystem.listFilesRecursive ../../nixfied/project)
        ++ (lib.filesystem.listFilesRecursive ../../tests/framework)
      );
  frameworkTestSources = builtins.filter (
    path: path != guardPath && lib.hasSuffix ".nix" (toString path)
  ) (lib.filesystem.listFilesRecursive ../../tests/framework);
  buildCheckSources = [
    ../../tests/framework/default.nix
  ];
  kernelSource = builtins.readFile ../../nixfied/framework/runtime/kernel/src/main.rs;
  readyHeliosSyncGateSource = builtins.readFile ../../tests/framework/ready-helios-sync-gate-smoke.nix;
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
  ) (frameworkSources ++ frameworkTestSources);
  disallowedBuildCheckJqFiles = builtins.filter (
    path: jqMarkerLines (builtins.readFile path) != [ ]
  ) buildCheckSources;
  disallowedAuthoredAppFiles = builtins.filter (
    path: lib.hasInfix "nixfied.apps" (builtins.readFile path)
  ) authoringSources;
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
  kernelSourceHasDeprecatedMarkers = lib.any (
    marker: lib.hasInfix marker kernelSource
  ) deprecatedKernelMarkers;
  deletedHelperPaths = [
    ../../nixfied/framework/core/machine-output-validate.py
    ../../nixfied/framework/core/introspection-query.py
    ../../nixfied/framework/contracts/mkValidator.nix
    ../../nixfied/framework/contracts/render-cue.nix
    ../../nixfied/framework/runtime/helpers/run-registry.nix
    ../../nixfied/framework/runtime/services/public-surface.nix
    ../../nixfied/modules/apps.nix
    ../../tests/framework/snapshots/contracts/example.cue
  ];
in
assert builtins.all (path: !builtins.pathExists path) deletedHelperPaths;
assert disallowedJqFiles == [ ];
assert disallowedPythonFiles == [ ];
assert disallowedBuildCheckJqFiles == [ ];
assert disallowedAuthoredAppFiles == [ ];
assert mismatchedAllowlistedFiles == [ ];
assert filesWithDeprecatedKernelMarkers == [ ];
assert !kernelSourceHasDeprecatedMarkers;
assert pkgs.lib.hasInfix "nixfied-kernel machine-output run" machineOutputSource;
assert !(pkgs.lib.hasInfix "validatorProgram" machineOutputSource);
assert !(pkgs.lib.hasInfix "python3" readyHeliosSyncGateSource);
assert !(pkgs.lib.hasInfix "http.server" readyHeliosSyncGateSource);
pkgs.runCommand "contract-migration-guard" { } ''
  echo "OK: hardening guards enforce deleted validators/CUE/run-registry/static service-surface/apps module, kernel-owned machine output, jq-free runtime/build-check seams, no authored nixfied.apps, no Python responders in framework/runtime tests, and no deprecated kernel seams in framework runtime or kernel source" > "$out"
''
