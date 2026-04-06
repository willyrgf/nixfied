{ pkgs, compiledExecution }:
let
  inherit (pkgs) lib;
  taskExecutionById = (compiledExecution.tasks or { }).byId or { };
  taskIds =
    compiledExecution.taskIds
      or (builtins.sort builtins.lessThan (builtins.attrNames taskExecutionById));
  taskHelpFiles = builtins.mapAttrs (
    taskId: taskExecution:
    pkgs.writeText "nixfied-task-help-${builtins.substring 0 10 (builtins.hashString "sha256" taskId)}.txt" ''
      ${builtins.concatStringsSep "\n" ((taskExecution.help or { }).lines or [ ])}
    ''
  ) taskExecutionById;
in
{
  inherit taskIds taskHelpFiles;

  taskHelpFileFor =
    taskId:
    if builtins.hasAttr taskId taskHelpFiles then
      builtins.toString taskHelpFiles.${taskId}
    else
      throw "missing compiled task help file for '${taskId}'";

  renderTaskHelpCases = builtins.concatStringsSep "\n" (
    map (taskId: ''
      ${lib.escapeShellArg taskId})
        cat ${lib.escapeShellArg (builtins.toString taskHelpFiles.${taskId})}
        return 0
        ;;
    '') taskIds
  );
}
