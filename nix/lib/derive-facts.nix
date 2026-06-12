# The pure derivation algorithms of docs/DERIVATION_SPEC.md (§3–§5), shared by
# the compiler (nix/compiler/derive.nix) and unit-tested against the spec's
# golden vectors (the `derive-facts-vectors` flake check). The runtime lowering
# re-derives the same facts at admission and fails closed on mismatch
# (DERIVE-1).
{ lib }:
rec {
  # Canonical order everywhere (spec §1): byte-wise ascending. Nix string `<`
  # compares byte-wise, matching Rust's BTreeSet iteration order.
  byteSort = builtins.sort (a: b: a < b);

  # The leaf task ids reachable from a task through composite steps (set
  # semantics — run-once duplication is irrelevant for the union). `tasks` is
  # the authored task attrset ({ kind, requires, steps, … }).
  leavesOf =
    tasks: taskId:
    if tasks.${taskId}.kind == "composite" then
      lib.unique (
        lib.concatMap (step: leavesOf tasks step.task) (builtins.attrValues tasks.${taskId}.steps)
      )
    else
      [ taskId ];

  # servicesRequired(task) — spec §3: the union of transitive leaf `requires`,
  # closed over `connectsTo`, byte-sorted and deduplicated.
  servicesRequired =
    { tasks, services }:
    taskId:
    let
      base = lib.unique (lib.concatMap (leaf: tasks.${leaf}.requires) (leavesOf tasks taskId));
      close =
        set:
        let
          next = lib.unique (set ++ lib.concatMap (service: services.${service}.connectsTo) set);
        in
        if builtins.length next == builtins.length set then set else close next;
    in
    byteSort (close base);

  # Default operation ids — spec §5.1.
  leafOperationId = name: "task.${name}.run";
  serviceOperationId = name: op: "service.${name}.${op}";

  # Default terminal tokens per lifecycle class — spec §5.2.
  terminalDefaults = {
    prepare = {
      success = "initialized";
      failure = "failed";
    };
    start = {
      success = "spawned";
      failure = "failed";
    };
    ready = {
      success = "ready";
      failure = "not-ready";
    };
    health = {
      success = "healthy";
      failure = "unhealthy";
    };
    stop = {
      success = "stopped";
      failure = "failed";
    };
    clean = {
      success = "cleaned";
      failure = "failed";
    };
  };

  # operationBindings(closure) — spec §4: the byte-sorted operation ids of
  # every invocation position whose run[0] resolves to the closure (the FIRST
  # tool whose executable basename equals run[0]); tool-set members that are
  # not the resolved executable bind nothing. `positions` is a list of
  # { operationId, toolIds, program }; `executableBasenames` maps closure id →
  # basename of its executable.
  operationBindings =
    { positions, executableBasenames }:
    closureId:
    byteSort (
      lib.unique (
        map (position: position.operationId) (
          builtins.filter (
            position:
            let
              matches = builtins.filter (
                id: (executableBasenames ? ${id}) && executableBasenames.${id} == position.program
              ) position.toolIds;
            in
            matches != [ ] && builtins.head matches == closureId
          ) positions
        )
      )
    );
}
