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
  kernelRustSources = builtins.filter (
    path: lib.hasSuffix ".rs" (toString path)
  ) (lib.filesystem.listFilesRecursive ../../nixfied/framework/runtime/kernel/src);
  kernelSource = lib.concatStringsSep "\n" (map builtins.readFile kernelRustSources);
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
  probePlanRuntimeSource = builtins.readFile ../../nixfied/framework/runtime/helpers/probe-plan-runtime.nix;
  executorSource = builtins.readFile ../../nixfied/framework/runtime/executor.nix;
  orchestratorSource = builtins.readFile ../../nixfied/framework/runtime/orchestrator.nix;
  orchestratorControlSource = builtins.readFile ../../nixfied/framework/runtime/orchestrator-control.nix;
  orchestratorRuntimeSource = builtins.readFile ../../nixfied/framework/runtime/orchestrator-runtime.nix;
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
assert pkgs.lib.hasInfix "nixfied-kernel probe evaluate" probePlanRuntimeSource;
assert !(pkgs.lib.hasInfix "jsonRpcHasResultCmd" probePlanRuntimeSource);
assert !(pkgs.lib.hasInfix "jsonRpcResultHexCmd" probePlanRuntimeSource);
assert !(pkgs.lib.hasInfix "jsonRpcResultCompactCmd" probePlanRuntimeSource);
assert !(pkgs.lib.hasInfix "jsonRpcResultFalseCmd" probePlanRuntimeSource);
assert !(pkgs.lib.hasInfix "pgIsReadyCmd" probePlanRuntimeSource);
assert !(pkgs.lib.hasInfix "psqlQueryCmd" probePlanRuntimeSource);
assert pkgs.lib.hasInfix "nixfied-kernel task execution-order" executorSource;
assert pkgs.lib.hasInfix "nixfied-kernel workflow serial-init" executorSource;
assert pkgs.lib.hasInfix "nixfied-kernel workflow serial-step" executorSource;
assert pkgs.lib.hasInfix "nixfied-kernel workflow parallel-init" executorSource;
assert pkgs.lib.hasInfix "nixfied-kernel workflow parallel-step" executorSource;
assert !(pkgs.lib.hasInfix "done < <(task_needs \"$current_task\")" executorSource);
assert !(pkgs.lib.hasInfix "done < <(task_soft_needs \"$current_task\")" executorSource);
assert pkgs.lib.hasInfix "nixfied-kernel summary collect-steps" executorSource;
assert !(pkgs.lib.hasInfix "workflow_step_status() {" executorSource);
assert !(pkgs.lib.hasInfix "workflow_step_records_tsv() {" executorSource);
assert !(pkgs.lib.hasInfix "workflow_peak_workers() {" executorSource);
assert !(pkgs.lib.hasInfix "mark_unit_canceled() {" executorSource);
assert !(pkgs.lib.hasInfix "cancel_pending_dependents() {" executorSource);
assert !(pkgs.lib.hasInfix "cancel_pending_units() {" executorSource);
assert !(pkgs.lib.hasInfix "unit_has_lock_conflict() {" executorSource);
assert !(pkgs.lib.hasInfix "assign_unit_locks() {" executorSource);
assert !(pkgs.lib.hasInfix "release_unit_locks() {" executorSource);
assert !(pkgs.lib.hasInfix "next_ready_unit() {" executorSource);
assert !(pkgs.lib.hasInfix "start_unit() {" executorSource);
assert !(pkgs.lib.hasInfix "cancel_running_units() {" executorSource);
assert !(pkgs.lib.hasInfix "UNIT_NEEDS_LEFT" executorSource);
assert !(pkgs.lib.hasInfix "UNIT_STATE" executorSource);
assert !(pkgs.lib.hasInfix "UNIT_DEPENDENTS" executorSource);
assert !(pkgs.lib.hasInfix "UNIT_LOCKS" executorSource);
assert !(pkgs.lib.hasInfix "LOCK_OWNER" executorSource);
assert !(pkgs.lib.hasInfix "blocked_tasks_by_dependency" executorSource);
assert !(pkgs.lib.hasInfix "blocked_tasks_reason_by_dependency" executorSource);
assert pkgs.lib.hasInfix "nixfied-kernel run-record read" orchestratorRuntimeSource;
assert !(pkgs.lib.hasInfix "\${pkgs.gnused}/bin/sed -n" orchestratorRuntimeSource);
assert pkgs.lib.hasInfix "nixfied-kernel registry terminal" orchestratorSource;
assert pkgs.lib.hasInfix "nixfied-kernel registry terminal" orchestratorControlSource;
assert
  !(pkgs.lib.hasInfix "while IFS=$'\\t' read -r seq ts_epoch ts event_run_id" orchestratorSource);
assert
  !(pkgs.lib.hasInfix "while IFS=$'\\t' read -r seq ts_epoch ts event_run_id" orchestratorControlSource);
assert !(pkgs.lib.hasInfix "python3" readyHeliosSyncGateSource);
assert !(pkgs.lib.hasInfix "http.server" readyHeliosSyncGateSource);
pkgs.runCommand "contract-migration-guard" { } ''
  echo "OK: hardening guards enforce deleted validators/CUE/run-registry/static service-surface/apps module, kernel-owned machine output/probe execution/executor task planning/executor serial scheduling/executor parallel scheduling/executor summary/orchestrator reads, jq-free runtime/build-check seams, no authored nixfied.apps, no Python responders in framework/runtime tests, and no deprecated kernel seams in framework runtime or kernel source" > "$out"
''
