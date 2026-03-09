{ pkgs, model }:
let
  source = builtins.readFile ../../nixfied/modules/operations.nix;
  probeRuntimeSource = builtins.readFile ../../nixfied/.framework/lib/operations-probe-runtime.nix;
  isolationRuntimeSource = builtins.readFile ../../nixfied/.framework/lib/test-isolation-runtime.nix;
  serviceConfigSource = builtins.readFile ../../nixfied/lib/service-config.nix;
  healthCommand = model.tasks."task.ops.health".runner.command;
  readyCommand = model.tasks."task.ops.ready".runner.command;
in
assert pkgs.lib.hasInfix "id = \"task.ops.health\";" source;
assert pkgs.lib.hasInfix "id = \"task.ops.ready\";" source;
assert pkgs.lib.hasInfix "name = \"service\";" source;
assert pkgs.lib.hasInfix "long = \"--service\";" source;
assert pkgs.lib.hasInfix "long = \"--source\";" source;
assert pkgs.lib.hasInfix "long = \"--slot\";" source;
assert pkgs.lib.hasInfix "long = \"--env\";" source;
assert pkgs.lib.hasInfix "long = \"--max-parallel\";" source;
assert pkgs.lib.hasInfix "service_selected()" source;
assert pkgs.lib.hasInfix "resolve_service_source()" source;
assert pkgs.lib.hasInfix "source_kind_disallowed()" source;
assert pkgs.lib.hasInfix "serviceConfigLib = import ../lib/service-config.nix" source;
assert pkgs.lib.hasInfix "probeRuntime = import ../.framework/lib/operations-probe-runtime.nix"
  source;
assert pkgs.lib.hasInfix "resolvedServiceConfigByName =" source;
assert pkgs.lib.hasInfix "resolveServicePortBase =" source;
assert pkgs.lib.hasInfix "probePlan =" source;
assert pkgs.lib.hasInfix "renderProbeStep = probeRuntime.renderProbeStep;" source;
assert pkgs.lib.hasInfix "renderProbeStep =" source;
assert pkgs.lib.hasInfix "mkServiceProbeSpec =" source;
assert pkgs.lib.hasInfix "healthProbeSpecs = builtins.listToAttrs" source;
assert pkgs.lib.hasInfix "readyProbeSpecs = builtins.listToAttrs" source;
assert pkgs.lib.hasInfix
  "testIsolationRuntime = import ../.framework/lib/test-isolation-runtime.nix"
  source;
assert pkgs.lib.hasInfix "isolationScript = testIsolationRuntime.mkIsolationScript" source;
assert pkgs.lib.hasInfix "mkTcpProbeBody =" probeRuntimeSource;
assert pkgs.lib.hasInfix "mkPostgresPgIsReadyBody =" probeRuntimeSource;
assert pkgs.lib.hasInfix "mkPostgresQueryBody =" probeRuntimeSource;
assert pkgs.lib.hasInfix "mkJsonRpcProbeBody =" probeRuntimeSource;
assert pkgs.lib.hasInfix "mkHeliosReadyBody =" probeRuntimeSource;
assert pkgs.lib.hasInfix "renderProbeStep =" probeRuntimeSource;
assert pkgs.lib.hasInfix "operationProbes = {" serviceConfigSource;
assert pkgs.lib.hasInfix "serviceLabel = \"postgres\";" serviceConfigSource;
assert pkgs.lib.hasInfix "kind = \"postgres-pg-isready\";" serviceConfigSource;
assert pkgs.lib.hasInfix "kind = \"postgres-query\";" serviceConfigSource;
assert pkgs.lib.hasInfix "kind = \"tcp\";" serviceConfigSource;
assert pkgs.lib.hasInfix "kind = \"jsonrpc\";" serviceConfigSource;
assert pkgs.lib.hasInfix "kind = \"helios-ready\";" serviceConfigSource;
assert pkgs.lib.hasInfix "serviceLabel = \"nginx\";" serviceConfigSource;
assert pkgs.lib.hasInfix "serviceLabel = \"minio\";" serviceConfigSource;
assert pkgs.lib.hasInfix "serviceLabel = \"reth\";" serviceConfigSource;
assert pkgs.lib.hasInfix "serviceLabel = \"helios execution\";" serviceConfigSource;
assert pkgs.lib.hasInfix "serviceLabel = \"helios\";" serviceConfigSource;
assert pkgs.lib.hasInfix "phaseLabel = \"health\";" serviceConfigSource;
assert pkgs.lib.hasInfix "phaseLabel = \"readiness\";" serviceConfigSource;
assert pkgs.lib.hasInfix "method = \"web3_clientVersion\";" serviceConfigSource;
assert pkgs.lib.hasInfix "method = \"eth_chainId\";" serviceConfigSource;
assert pkgs.lib.hasInfix "INFO: checking helios readiness" readyCommand;
assert pkgs.lib.hasInfix "source kind disallowed" readyCommand;
assert pkgs.lib.hasInfix "OK: readiness checks passed" readyCommand;
assert pkgs.lib.hasInfix "OK: health checks passed" healthCommand;
assert pkgs.lib.hasInfix "isolation cell start" isolationRuntimeSource;
assert pkgs.lib.hasInfix "test-isolation logs_root=" isolationRuntimeSource;
assert pkgs.lib.hasInfix "test-isolation forcing maxParallel=1 reason=ci" isolationRuntimeSource;
assert pkgs.lib.hasInfix "\"$executor_bin\" run-task \"$validate_task_id\"" isolationRuntimeSource;
assert pkgs.lib.hasInfix "\"$executor_bin\" run-task \"$run_task_id\"" isolationRuntimeSource;
assert pkgs.lib.hasInfix "test-isolation matrix has no slots" isolationRuntimeSource;
assert pkgs.lib.hasInfix "test-isolation completed with failures" isolationRuntimeSource;
pkgs.runCommand "operations-contract" { } ''
    cat > task-ops-health.sh <<'EOF'
  ${healthCommand}
  EOF
    cat > task-ops-ready.sh <<'EOF'
  ${readyCommand}
  EOF
    ${pkgs.bash}/bin/bash -n task-ops-health.sh
    ${pkgs.bash}/bin/bash -n task-ops-ready.sh
    echo "OK: operations health/readiness contract markers are stable" > "$out"
''
