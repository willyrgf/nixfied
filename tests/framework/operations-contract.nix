{ pkgs, model }:
let
  source = builtins.readFile ../../nixfied/modules/operations.nix;
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
assert pkgs.lib.hasInfix "resolvedServiceConfigByName =" source;
assert pkgs.lib.hasInfix "resolveServicePortBase =" source;
assert pkgs.lib.hasInfix "probePlan =" source;
assert pkgs.lib.hasInfix "renderProbeStep =" source;
assert pkgs.lib.hasInfix "mkServiceProbeSpec =" source;
assert pkgs.lib.hasInfix "mkTcpProbeBody =" source;
assert pkgs.lib.hasInfix "mkPostgresPgIsReadyBody =" source;
assert pkgs.lib.hasInfix "mkPostgresQueryBody =" source;
assert pkgs.lib.hasInfix "mkJsonRpcProbeBody =" source;
assert pkgs.lib.hasInfix "mkHeliosReadyBody =" source;
assert pkgs.lib.hasInfix "healthProbeSpecs = builtins.listToAttrs" source;
assert pkgs.lib.hasInfix "readyProbeSpecs = builtins.listToAttrs" source;
assert pkgs.lib.hasInfix "checking helios readiness" source;
assert pkgs.lib.hasInfix "method = \"eth_blockNumber\";" source;
assert pkgs.lib.hasInfix "method = \"eth_syncing\";" source;
assert pkgs.lib.hasInfix "source kind disallowed" source;
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
assert pkgs.lib.hasInfix "isolation cell start" source;
assert pkgs.lib.hasInfix "test-isolation logs_root=" source;
assert pkgs.lib.hasInfix "test-isolation forcing maxParallel=1 reason=ci" source;
assert pkgs.lib.hasInfix "\"$executor_bin\" run-task \"$validate_task_id\"" source;
assert pkgs.lib.hasInfix "\"$executor_bin\" run-task \"$run_task_id\"" source;
assert pkgs.lib.hasInfix "test-isolation matrix has no slots" source;
assert pkgs.lib.hasInfix "test-isolation completed with failures" source;
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
