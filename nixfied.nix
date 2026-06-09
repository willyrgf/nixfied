# The framework's own project.
#
# A "CI job" in nixfied's vocabulary is a task; a "CI pipeline" is a workflow. So
# nixfied's end-to-end gate is the product running itself: this repo is a nixfied
# project whose `conformance` workflow exercises the framework end to end, run by
# the very `nixfied-runtime` under test and invoked through the same surfaces an
# adopter uses. No bespoke harness interface.
#
# Each node is a task bound to the nix-built `nixfied-conformance` closure with a
# `--check <name>` selecting one capability check (plus an adoption check that
# installs + upgrades a throwaway project). The capability checks drive a
# nix-built example model store path through the nix-built runtime; the conformance
# closure asserts on operator-observable outputs and writes a ground-truth
# artifact per check.
#
# Run the gate:  nixfied-runtime run --model <store>/model.json --workflow conformance
{
  pkgs,
  system,
  adapters,
  ...
}:
let
  # Compile an example model to its store output (its model.json is baked into
  # the conformance task args, so capability checks need no nix at run time).
  compile =
    module:
    import ./nix/compiler/default.nix {
      lib = pkgs.lib;
      inherit pkgs system module;
    };
  models = {
    minimal = compile ./examples/minimal/nixfied.nix;
    postgres = compile ./examples/postgres/nixfied.nix;
    workflow = compile ./examples/workflow/nixfied.nix;
    polyglot = compile ./examples/polyglot-stack/nixfied.nix;
    downstream = compile ./examples/downstream/nixfied.nix;
  };

  # The nix-built runtime/conformance binaries. The conformance task runs
  # `bin/nixfied-conformance`, which locates `bin/nixfied-runtime` as its sibling.
  runtime = import ./nix/packages/runtime.nix { inherit pkgs; };

  conformanceCheck = name: model: {
    operationId = "task.conformance.${name}.run";
    execId = "conformance";
    args =
      [
        "--check"
        name
      ]
      ++ pkgs.lib.optionals (model != null) [
        "--model"
        "${model}/model.json"
      ];
    dependsOnServicesReady = [ "synthetic" ];
    logRefs = [ "task.conformance.${name}" ];
  };
in
{
  # The synthetic adapter provides a tiny anchor service the conformance tasks
  # attach to (every task is gated on a ready service). The conformance tasks
  # ignore its endpoint; their subjects are the inner runs they spawn.
  imports = [ adapters.synthetic ];

  nixfied.project.projectId = "nixfied";
  nixfied.project.name = "Nixfied Self-Conformance";
  nixfied.codebases.main.logicalRoot = ".";

  # A candidate window disjoint from every example window, so the outer (anchor)
  # run never collides with the inner subject runs (which use their own windows).
  nixfied.placement.ports.base = 38500;

  # The conformance closure: the nix-built workspace binaries.
  nixfied.closures.conformance = {
    package = runtime;
    executable = "bin/nixfied-conformance";
    operationBindings = [
      "task.conformance.minimal.run"
      "task.conformance.postgres.run"
      "task.conformance.workflow.run"
      "task.conformance.polyglot.run"
      "task.conformance.downstream.run"
      "task.conformance.slots.run"
      "task.conformance.adoption.run"
      "task.conformance.negative.run"
    ];
    effects = [
      "process"
      "network-listener"
      "file-write"
    ];
  };

  nixfied.execs.conformance = {
    closureId = "conformance";
    # Each check spawns an inner runtime (Postgres init/start, an install/build),
    # so the task budget is generous.
    timeoutMs = 900000;
  };

  nixfied.tasks.check-minimal = conformanceCheck "minimal" models.minimal;
  nixfied.tasks.check-postgres = conformanceCheck "postgres" models.postgres;
  nixfied.tasks.check-workflow = conformanceCheck "workflow" models.workflow;
  nixfied.tasks.check-polyglot = conformanceCheck "polyglot" models.polyglot;
  nixfied.tasks.check-downstream = conformanceCheck "downstream" models.downstream;
  # Slot isolation: two slots of the multi-slot downstream model run concurrently.
  nixfied.tasks.check-slots = conformanceCheck "slots" models.downstream;
  # The adoption check scaffolds a throwaway git repo and runs the real
  # `#install` + `#upgrade` against `path:<checkout>`; it derives the checkout
  # from its working directory, so no example model is baked in.
  nixfied.tasks.check-adoption = conformanceCheck "adoption" null;
  # The standing proof that the gate distinguishes pass from fail: this check
  # expects an inner run to fail (an undeclared workflow). If the runtime instead
  # accepted it, the task exits non-zero and fails the workflow with the reason.
  nixfied.tasks.check-negative = conformanceCheck "negative" models.minimal;

  # The pipeline. Nodes are chained so the inner runs execute one at a time, with
  # the cheap launch-sanity checks first and adoption (the slowest) last.
  nixfied.workflows.conformance = {
    servicesRequired = [ "synthetic" ];
    nodes = [
      {
        nodeId = "minimal";
        taskId = "check-minimal";
        dependsOn = [ ];
      }
      {
        nodeId = "workflow";
        taskId = "check-workflow";
        dependsOn = [ "minimal" ];
      }
      {
        nodeId = "polyglot";
        taskId = "check-polyglot";
        dependsOn = [ "workflow" ];
      }
      {
        nodeId = "postgres";
        taskId = "check-postgres";
        dependsOn = [ "polyglot" ];
      }
      {
        nodeId = "downstream";
        taskId = "check-downstream";
        dependsOn = [ "postgres" ];
      }
      {
        nodeId = "slots";
        taskId = "check-slots";
        dependsOn = [ "downstream" ];
      }
      {
        nodeId = "negative";
        taskId = "check-negative";
        dependsOn = [ "slots" ];
      }
      {
        nodeId = "adoption";
        taskId = "check-adoption";
        dependsOn = [ "negative" ];
      }
    ];
  };
}
