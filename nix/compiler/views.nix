{ model }:

let
  inherit (builtins) attrNames concatStringsSep;

  inlineList =
    values:
    if values == [ ] then
      "none"
    else
      concatStringsSep ", " (map (value: "`${toString value}`") values);

  taskDocs =
    taskId:
    let
      task = model.tasks.${taskId};
      stepNames = attrNames (task.steps or { });
    in
    concatStringsSep "\n" (
      [
        "### `${taskId}`"
        ""
        "- kind: `${task.kind}`"
        "- service lifetime: `${task.serviceLifetime}`"
        "- services required: ${inlineList task.servicesRequired}"
        "- artifact refs: ${inlineList (task.artifactRefs or [ ])}"
        "- log refs: ${inlineList (task.logRefs or [ ])}"
        "- summary refs: ${inlineList (task.summaryRefs or [ ])}"
      ]
      ++ (
        if task.kind == "composite" then
          [ "- steps:" ]
          ++ map (
            stepName:
            let
              step = task.steps.${stepName};
            in
            "  - `${stepName}`: task `${toString step.task}`; depends on ${inlineList (step.dependsOn or [ ])}"
          ) stepNames
        else
          [ ]
      )
    );

  serviceDocs =
    serviceId:
    let
      service = model.services.${serviceId};
      endpointNames = attrNames (service.endpoints or { });
    in
    concatStringsSep "\n" (
      [
        "### `${serviceId}`"
        ""
        "- primary endpoint: ${
          if (service.primaryEndpoint or null) == null then "none" else "`${service.primaryEndpoint}`"
        }"
        "- connects to: ${inlineList service.connectsTo}"
        "- containment: `${service.containment}`"
        "- state refs: ${inlineList service.stateRefs}"
        "- log refs: ${inlineList service.logRefs}"
        "- endpoints:"
      ]
      ++ (
        if endpointNames == [ ] then
          [ "  - none" ]
        else
          map (
            endpointName:
            let
              endpoint = service.endpoints.${endpointName};
              primary = service.primaryEndpoint or null;
            in
            "  - `${endpointName}`: `${endpoint.host}`${if primary == endpointName then " (primary)" else ""}"
          ) endpointNames
      )
    );

  taskNames = attrNames model.tasks;
  serviceNames = attrNames model.services;
  slotNames = attrNames model.placement.slotPlacements;
  slotWindows = concatStringsSep "\n" (
    map (
      slotName:
      let
        placement = model.placement.slotPlacements.${slotName};
      in
      "  - slot `${toString placement.slot}`: `${toString placement.candidatePorts.start}` through `${
        toString placement.candidatePorts.end
      }`"
    ) slotNames
  );
in
''
  # ${model.project.name}

  ## Project

  - id: `${model.project.projectId}`
  - target system: `${model.target.system}`
  - runtime ABI: `${model.runtimeAbi}`
  - toolchain: `${model.toolchainId}`

  ## Slots

  - allowed: `${toString model.slotPolicy.min}` through `${toString model.slotPolicy.max}`
  - default: `${toString model.slotPolicy.default}`
  - candidate port windows:
  ${slotWindows}

  ## State

  - marker identity: `${model.state.markerIdentity}`
  - epoch: `${model.state.stateEpoch}`
  - cleanup: `${model.state.cleanupPolicy}`
  - persistence: `${model.state.persistence}`

  ## Tasks

  ${if taskNames == [ ] then "none" else concatStringsSep "\n\n" (map taskDocs taskNames)}

  ## Services

  ${if serviceNames == [ ] then "none" else concatStringsSep "\n\n" (map serviceDocs serviceNames)}
''
