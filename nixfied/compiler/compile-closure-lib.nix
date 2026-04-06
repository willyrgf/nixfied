# Shared closure-walking helpers for the compiler.
#
# compile-execution.nix traverses the same task/workflow graph for both
# service-name closures and task/workflow-id closures. This module extracts
# the common pieces:
#
#   * workflowFamilyFromId      -- parse "workflow.<family>.<mode>" ids
#   * workflowIdsByFamily       -- map each workflowId to sibling ids
#   * mkClosureWalker           -- parameterised graph walker
#
# mkClosureWalker accepts a `strategy` attrset that controls what each
# graph node contributes and how results are merged.  See the inline
# documentation below for the full strategy interface.
#
_:
{
  tasks,
  workflows,
  ...
}:
let
  listUtils = import ../framework/core/list-utils.nix;
  inherit (listUtils) uniquePreserveOrder;

  taskSet = if tasks == null then { } else tasks;
  workflowSet = if workflows == null then { } else workflows;

  workflowIds = builtins.sort builtins.lessThan (builtins.attrNames workflowSet);

  # ---- shared helpers ------------------------------------------------

  workflowFamilyFromId =
    workflowId:
    let
      match = builtins.match "^workflow\\.([^.]+)\\..+$" workflowId;
    in
    if match == null then null else builtins.elemAt match 0;

  # Map every workflowId to the list of sibling ids that share its family.
  # Used by goWorkflowReference to fan out across all modes of a family.
  workflowIdsByFamily = builtins.listToAttrs (
    map (
      workflowId:
      let
        family = workflowFamilyFromId workflowId;
      in
      {
        name = workflowId;
        value =
          if family == null then
            [ workflowId ]
          else
            builtins.filter (candidateId: workflowFamilyFromId candidateId == family) workflowIds;
      }
    ) workflowIds
  );

  # ---- generic closure walker ----------------------------------------
  #
  # `strategy` is an attrset with the following fields:
  #
  #   empty : value
  #       The zero/identity value returned when a node is skipped (cycle
  #       or missing).  For service-name closures this is []; for
  #       task/workflow-id closures it is { taskIds=[]; workflowIds=[]; }.
  #
  #   merge : list-of-results -> result
  #       Combine a list of results from sub-traversals into one result.
  #
  #   taskContrib : taskId -> task -> result
  #       The leaf contribution of a single task before any recursion.
  #       For service closures: task.requirements.services;
  #       for id closures: { taskIds = [ taskId ]; workflowIds = []; }.
  #
  #   taskWorkflowRef : seen -> task -> goWorkflowReference -> result
  #       Handle the case where a task's runner is a workflowRef.
  #       Receives the current seen-set, the task, and the
  #       goWorkflowReference function to call.
  #
  #   taskRuntimeWorkflows : seen -> task -> goWorkflowExact -> result
  #       Handle runtime.references.workflowIds on a task.
  #
  #   workflowUnit : seen -> unit -> goTask -> result
  #       Process one workflow unit.
  #
  #   workflowPhase : seen -> workflow -> goTask -> result
  #       Process preRun/postRun phase tasks of a workflow.
  #
  #   workflowPhaseServiceSets : workflow -> result
  #       Collect any phase service-set contributions from a workflow.
  #
  #   workflowSelf : workflowId -> innerResult -> result
  #       Wrap the combined inner result with the workflow's own
  #       contribution (e.g. prepend the workflowId itself).
  #
  mkClosureWalker =
    strategy:
    let
      goTask =
        seen: taskId:
        let
          token = "task:${taskId}";
        in
        if !(builtins.hasAttr taskId taskSet) || builtins.elem token seen then
          strategy.empty
        else
          let
            task = taskSet.${taskId};
            nextSeen = seen ++ [ token ];
            depIds = (task.deps.needs or [ ]) ++ (task.deps.softNeeds or [ ]);
            depResults = map (depTaskId: goTask nextSeen depTaskId) depIds;
            runtimeTaskResults = map (refTaskId: goTask nextSeen refTaskId) (
              task.runtime.references.taskIds or [ ]
            );
            runtimeWorkflowResults = strategy.taskRuntimeWorkflows nextSeen task goWorkflowExact;
            workflowRefResult = strategy.taskWorkflowRef nextSeen task goWorkflowReference;
          in
          strategy.merge [
            (strategy.taskContrib taskId task)
            (strategy.merge depResults)
            (strategy.merge runtimeTaskResults)
            runtimeWorkflowResults
            workflowRefResult
          ];

      goWorkflow =
        seen: workflowId:
        let
          token = "workflow:${workflowId}";
        in
        if !(builtins.hasAttr workflowId workflowSet) || builtins.elem token seen then
          strategy.empty
        else
          let
            workflow = workflowSet.${workflowId};
            nextSeen = seen ++ [ token ];
            unitNames = builtins.sort builtins.lessThan (builtins.attrNames (workflow.units or { }));
            unitResults = map (
              unitName: strategy.workflowUnit nextSeen workflow.units.${unitName} goTask
            ) unitNames;
            phaseResult = strategy.workflowPhase nextSeen workflow goTask;
            phaseServiceSetResult = strategy.workflowPhaseServiceSets workflow;
            inner = strategy.merge (
              unitResults
              ++ [
                phaseResult
                phaseServiceSetResult
              ]
            );
          in
          strategy.workflowSelf workflowId inner;

      goWorkflowExact = seen: workflowId: goWorkflow seen workflowId;

      goWorkflowReference =
        seen: workflowId:
        let
          workflowIdsForReference = workflowIdsByFamily.${workflowId} or [ workflowId ];
          results = map (candidateId: goWorkflow seen candidateId) workflowIdsForReference;
        in
        strategy.merge results;
    in
    {
      inherit
        goTask
        goWorkflow
        goWorkflowExact
        goWorkflowReference
        ;
    };
in
{
  inherit
    workflowFamilyFromId
    workflowIdsByFamily
    uniquePreserveOrder
    mkClosureWalker
    ;
}
