{
  lib,
  canonical,
}:
{
  resolved,
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
      taskId = raw.taskId or "";
      task = if taskId != "" && builtins.hasAttr taskId tasks then tasks.${taskId} else null;
    in
    if appId == "" then
      throw "nixfied apps: app '${name}' must have a non-empty id"
    else if (raw.kind or "taskRef") != "taskRef" then
      throw "nixfied apps: unsupported app kind '${raw.kind}' for '${appId}'"
    else if taskId == "" then
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
      };

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
