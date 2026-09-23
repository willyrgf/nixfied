{ manifest }:

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
      task = manifest.tasks.${taskId};
      stepNames = attrNames (task.steps or { });
    in
    concatStringsSep "\n" (
      [
        "### `${taskId}`"
        ""
        "- kind: `${task.kind}`"
        "- default output: `${task.defaultOutput}`"
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
      service = manifest.services.${serviceId};
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

  taskNames = attrNames manifest.tasks;
  serviceNames = attrNames manifest.services;
  slotNames = attrNames manifest.placement.slotPlacements;
  slotWindows = concatStringsSep "\n" (
    map (
      slotName:
      let
        placement = manifest.placement.slotPlacements.${slotName};
      in
      "  - slot `${toString placement.slot}`: `${toString placement.candidatePorts.start}` through `${
        toString placement.candidatePorts.end
      }`"
    ) slotNames
  );
in
''
  # ${manifest.project.name}

  ## Project

  - id: `${manifest.project.projectId}`
  - target system: `${manifest.target.system}`
  - runtime ABI: `${manifest.runtimeAbi}`
  - toolchain: `${manifest.toolchainId}`

  ## Slots

  - allowed: `${toString manifest.slotPolicy.min}` through `${toString manifest.slotPolicy.max}`
  - default: `${toString manifest.slotPolicy.default}`
  - candidate port windows:
  ${slotWindows}

  ## State

  - marker identity: `${manifest.state.markerIdentity}`
  - epoch: `${manifest.state.stateEpoch}`
  - cleanup: `${manifest.state.cleanupPolicy}`
  - persistence: `${manifest.state.persistence}`

  ## Tasks

  ${if taskNames == [ ] then "none" else concatStringsSep "\n\n" (map taskDocs taskNames)}

  ## Services

  ${if serviceNames == [ ] then "none" else concatStringsSep "\n\n" (map serviceDocs serviceNames)}
''
