# Nix-side composition sugar over the task algebra. The contract carries only
# the closed DAG form (STATIC-1); these helpers expand to it at authoring time.
{ lib }:
{
  # A sequential chain of task references as a composite `steps` attrset: each
  # step is named after its task and depends on the previous one. Tasks must be
  # distinct (name a repeated task manually).
  seq =
    taskIds:
    assert lib.assertMsg (
      lib.unique taskIds == taskIds
    ) "lib.seq: task ids must be distinct (name repeated steps manually)";
    builtins.listToAttrs (
      lib.imap0 (index: taskId: {
        name = taskId;
        value = {
          task = taskId;
          dependsOn = lib.optional (index > 0) (builtins.elemAt taskIds (index - 1));
        };
      }) taskIds
    );
}
