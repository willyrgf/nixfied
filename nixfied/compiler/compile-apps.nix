{
  lib,
  canonical,
}:
{
  resolved,
  
  serviceSurfaceCatalog ? { },
  tasks,
  workflows,
  contractBundle,
  ...
}:
let
  rawMachineOutputs = resolved.machineOutputs or { };
  machineOutputNames = builtins.sort builtins.lessThan (builtins.attrNames rawMachineOutputs);
  taskIds = builtins.sort builtins.lessThan (builtins.attrNames tasks);
  workflowIds = builtins.sort builtins.lessThan (builtins.attrNames workflows);

  normalizeUsage = appId: values: if values != [ ] then values else [ "nix run .#${appId}" ];

  normalizeExamples = values: if values == [ ] then [ ] else values;
  normalizeOwnerFile = value: if value == null || value == "" then null else value;
  listUtils = import ../framework/core/list-utils.nix;
  normalizeAppRefs = listUtils.uniqueSorted;

  normalizeTaskLauncher =
    taskId:
    let
      task = tasks.${taskId};
      launcher = task.launcher or { };
      commandApi = task.commandApi or { };
      appId = launcher.appId or "";
    in
    if !(launcher.enable or false) then
      null
    else if appId == "" then
      throw "nixfied launchers: task '${taskId}' must set launcher.appId when launcher.enable=true"
    else
      canonical.canonicalize {
        id = appId;
        kind = "taskRef";
        inherit taskId;
        summary =
          if (launcher.summary or "") != "" then
            launcher.summary
          else
            commandApi.summary or task.summary or appId;
        description =
          if (launcher.description or "") != "" then
            launcher.description
          else
            commandApi.details or task.description or "";
        category = launcher.category or commandApi.category or "core";
        usage = normalizeUsage appId (launcher.usage or commandApi.usage or [ ]);
        examples = normalizeExamples (launcher.examples or commandApi.examples or [ ]);
        ownerFile = normalizeOwnerFile (launcher.ownerFile or null);
      };

  normalizeWorkflowLauncher =
    workflowId:
    let
      workflow = workflows.${workflowId};
      launcher = workflow.launcher or { };
      appId = launcher.appId or "";
    in
    if !(launcher.enable or false) then
      null
    else if appId == "" then
      throw "nixfied launchers: workflow '${workflowId}' must set launcher.appId when launcher.enable=true"
    else
      canonical.canonicalize {
        id = appId;
        kind = "workflowRef";
        inherit workflowId;
        summary = if (launcher.summary or "") != "" then launcher.summary else workflow.summary or appId;
        description =
          if (launcher.description or "") != "" then launcher.description else workflow.description or "";
        category = launcher.category or "core";
        usage = normalizeUsage appId (launcher.usage or [ ]);
        examples = normalizeExamples (launcher.examples or [ ]);
        ownerFile = normalizeOwnerFile (launcher.ownerFile or null);
      };

  serviceOperationApps = builtins.attrValues (serviceSurfaceCatalog.appsByName or { });

  generatedApps =
    (builtins.filter (app: app != null) (map normalizeTaskLauncher taskIds))
    ++ (builtins.filter (app: app != null) (map normalizeWorkflowLauncher workflowIds))
    ++ serviceOperationApps;

  generatedPreviewAppsById = builtins.listToAttrs (
    map (app: {
      name = app.id;
      value = {
        inherit (app) kind;
        taskId = app.taskId or "";
        workflowId = app.workflowId or "";
        serviceSetId = app.serviceSetId or "";
      };
    }) generatedApps
  );

  machineOutputPreviewAppsById = builtins.foldl' (
    acc: name:
    let
      raw = rawMachineOutputs.${name};
      appId = if (raw.id or "") == "" then name else raw.id;
    in
    acc
    // {
      ${appId} = {
        kind = "machineOutput";
        targetAppId = raw.targetAppId or "";
      };
    }
  ) { } machineOutputNames;

  previewAppsById = generatedPreviewAppsById // machineOutputPreviewAppsById;
  appExists = appId: builtins.hasAttr appId previewAppsById;
  appPreview = appId: previewAppsById.${appId};
  isMachineOutputPreview = appId: appExists appId && (appPreview appId).kind == "machineOutput";

  normalizeMachineOutput =
    name:
    let
      raw = rawMachineOutputs.${name};
      appId = if (raw.id or "") == "" then name else raw.id;
      targetAppId = raw.targetAppId or "";
      targetArgs = raw.targetArgs or [ ];
      setupAppIds = normalizeAppRefs (raw.setupAppIds or [ ]);
      teardownAppIds = normalizeAppRefs (raw.teardownAppIds or [ ]);
      contractRef = (raw.validation or { }).contractRef or "";
      validationSchema =
        if contractRef == "" then
          null
        else if builtins.hasAttr contractRef contractBundle.validationSchemas then
          contractBundle.validationSchemas.${contractRef}
        else
          throw "nixfied machineOutputs: '${appId}' references unknown contractRef '${contractRef}'";
      targetPreview = if targetAppId != "" && appExists targetAppId then appPreview targetAppId else null;
      referencedAppIds =
        setupAppIds ++ teardownAppIds ++ lib.optionals (targetAppId != "") [ targetAppId ];
      unknownReferencedAppIds = builtins.filter (appRef: !(appExists appRef)) referencedAppIds;
      nestedMachineOutputRefs = builtins.filter isMachineOutputPreview referencedAppIds;
      machineSummary =
        if targetPreview == null then "Machine output ${appId}" else "Machine output ${targetAppId}";
    in
    if appId == "" then
      throw "nixfied machineOutputs: '${name}' must have a non-empty id"
    else if targetAppId == "" then
      throw "nixfied machineOutputs: '${appId}' must set targetAppId"
    else if contractRef == "" then
      throw "nixfied machineOutputs: '${appId}' must set validation.contractRef"
    else if targetAppId == appId then
      throw "nixfied machineOutputs: '${appId}' cannot target itself"
    else if unknownReferencedAppIds != [ ] then
      throw "nixfied machineOutputs: '${appId}' references unknown apps: ${builtins.concatStringsSep ", " unknownReferencedAppIds}"
    else if nestedMachineOutputRefs != [ ] then
      throw "nixfied machineOutputs: '${appId}' cannot reference machineOutput apps: ${builtins.concatStringsSep ", " nestedMachineOutputRefs}"
    else
      canonical.canonicalize {
        id = appId;
        kind = "machineOutput";
        inherit targetAppId;
        inherit targetArgs;
        inherit setupAppIds;
        inherit teardownAppIds;
        validation = {
          inherit contractRef;
        }
        // lib.optionalAttrs (validationSchema != null) {
          schema = validationSchema;
        };
        summary = if (raw.summary or "") != "" then raw.summary else machineSummary;
        description =
          if (raw.description or "") != "" then
            raw.description
          else
            "Runs '${targetAppId}' behind a strict JSON output contract.";
        category = raw.category or "core";
        usage = normalizeUsage appId (raw.usage or [ ]);
        examples = normalizeExamples (raw.examples or [ ]);
        ownerFile = normalizeOwnerFile (raw.ownerFile or null);
      };

  machineOutputs = map normalizeMachineOutput machineOutputNames;

  addApp =
    acc: app:
    let
      appId = app.id;
    in
    if builtins.hasAttr appId acc then
      throw "nixfied launchers: duplicate app id '${appId}'"
    else
      acc
      // {
        ${appId} = app;
      };
in
builtins.foldl' addApp { } (generatedApps ++ machineOutputs)
