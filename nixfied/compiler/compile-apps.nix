{
  lib,
  canonical,
}:
{
  resolved,
  serviceSets,
  tasks,
}:
let
  rawApps = resolved.apps or { };
  names = builtins.sort builtins.lessThan (builtins.attrNames rawApps);

  normalizeApp =
    name:
    let
      raw = rawApps.${name};
      appId = if (raw.id or "") == "" then name else raw.id;
      kind = raw.kind or "taskRef";
      taskId = raw.taskId or "";
      task = if taskId != "" && builtins.hasAttr taskId tasks then tasks.${taskId} else null;
      serviceSetId = raw.serviceSetId or "";
      serviceSet =
        if serviceSetId != "" && builtins.hasAttr serviceSetId serviceSets then
          serviceSets.${serviceSetId}
        else
          null;
      operation = raw.operation or "";
      operationSummary =
        if serviceSet == null then
          ""
        else
          "${serviceSet.summary} ${operation}";
      operationDescription =
        if serviceSet == null then
          ""
        else if operation == "export" then
          "Prints per-service handoff data for '${serviceSet.name}'."
        else
          "Runs '${operation}' for the '${serviceSet.name}' service set.";
    in
    if appId == "" then
      throw "nixfied apps: app '${name}' must have a non-empty id"
    else if kind == "taskRef" then
      if taskId == "" then
        throw "nixfied apps: taskRef app '${appId}' must set taskId"
      else if task == null then
        throw "nixfied apps: app '${appId}' references unknown task '${taskId}'"
      else
        canonical.canonicalize {
          id = appId;
          kind = "taskRef";
          taskId = taskId;
          summary = if (raw.summary or "") != "" then raw.summary else task.summary or appId;
          description = if (raw.description or "") != "" then raw.description else task.description or "";
          category = raw.category or "core";
          usage = raw.usage or [ ];
          examples = raw.examples or [ ];
          ownerFile = if (raw.ownerFile or null) == null || raw.ownerFile == "" then null else raw.ownerFile;
        }
    else if kind == "serviceSetRef" then
      if serviceSetId == "" then
        throw "nixfied apps: serviceSetRef app '${appId}' must set serviceSetId"
      else if serviceSet == null then
        throw "nixfied apps: app '${appId}' references unknown service set '${serviceSetId}'"
      else if !(builtins.elem operation [
        "start"
        "stop"
        "status"
        "health"
        "ready"
        "export"
      ]) then
        throw "nixfied apps: serviceSetRef app '${appId}' must use a supported operation"
      else
        canonical.canonicalize {
          id = appId;
          kind = "serviceSetRef";
          serviceSetId = serviceSetId;
          operation = operation;
          summary = if (raw.summary or "") != "" then raw.summary else operationSummary;
          description = if (raw.description or "") != "" then raw.description else operationDescription;
          category = raw.category or "core";
          usage = raw.usage or [ ];
          examples = raw.examples or [ ];
          ownerFile = if (raw.ownerFile or null) == null || raw.ownerFile == "" then null else raw.ownerFile;
        }
    else
      throw "nixfied apps: unsupported app kind '${kind}' for '${appId}'";

  addApp =
    acc: name:
    let
      app = normalizeApp name;
      appId = app.id;
    in
    if builtins.hasAttr appId acc then
      throw "nixfied apps: duplicate app id '${appId}'"
    else
      acc
      // {
        ${appId} = app;
      };
in
builtins.foldl' addApp { } names
