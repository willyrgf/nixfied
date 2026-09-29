# The single traversal of authored invocation positions. Consumers validate
# references, derive tool closures, and check package availability from this list.
{ lib, config }:
let
  facts = import ../lib/derive-facts.nix { inherit lib; };
  operation = name: phase: op:
    if op.operationId != null then op.operationId else facts.serviceOperationId name phase;
in
lib.concatLists (lib.mapAttrsToList (name: service:
  let
    lc = service.lifecycle;
    endpoints = if service.endpoint != null then
      { ${service.endpoint.endpointId} = service.endpoint; }
    else service.endpoints;
    position = phase: endpointId: invocation: {
      serviceName = name;
      inherit phase endpointId invocation;
      operationId = operation name phase lc.${phase};
    };
    probes = phase:
      lib.optional (lc.${phase}.probe != null) (position phase null lc.${phase}.probe)
      ++ lib.mapAttrsToList (id: endpoint: position phase id endpoint.${phase + "Probe"}) endpoints;
  in
  [ (position "start" null lc.start.invocation) ] ++ probes "ready" ++ probes "health"
) config.nixfied.services)
++ lib.mapAttrsToList (name: task: {
  serviceName = null;
  endpointId = null;
  phase = "task";
  operationId = if task.operationId != null then task.operationId else facts.leafOperationId name;
  invocation = task.invocation;
}) (lib.filterAttrs (_: task: task.kind == "leaf") config.nixfied.tasks)
