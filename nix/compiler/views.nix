{ model }:

{
  schema = {
    schemaVersion = 1;
    source = "model.json";
    runtimeInputs = model.runtimeConstraints;
    surfaces = model.surfaces;
    modelTypes = {
      modelVersion = model.modelVersion;
      runtimeAbi = model.runtimeAbi;
      toolchainId = model.toolchainId;
      primitives = [
        "ExecSpec"
        "EndpointSpec"
        "ProbeSpec"
        "LifecycleOpSpec"
        "LifecycleOpClass"
        "TerminalSemantics"
        "HealthPolicy"
        "ServiceSpec"
        "TaskSpec"
        "SlotPlacement"
      ];
    };
  };

  capabilities = model.capabilities;

  docs = ''
    # ${model.docs.title}

    ${model.docs.summary}

    ## Target

    - system: ${model.target.system}
    - runtime ABI: ${model.runtimeAbi}
    - toolchain: ${model.toolchainId}

    ## Surfaces

    ${builtins.concatStringsSep "\n" (map (surface: "- ${surface.name}") model.surfaces)}

    ## Services

    ${builtins.concatStringsSep "\n" (map (service: "- ${service}") model.capabilities.services)}

    ## Lifecycle

    ${builtins.concatStringsSep "\n" (
      map (
        service:
        let
          serviceSpec = model.services.${service};
          classes = map (op: op.class) serviceSpec.lifecycle;
        in
        "- ${service}: readiness ${serviceSpec.readinessProbe}; health ${serviceSpec.healthPolicy}; operations ${builtins.concatStringsSep ", " classes}"
      ) model.capabilities.services
    )}

    ## Tasks

    ${builtins.concatStringsSep "\n" (map (task: "- ${task}") model.capabilities.tasks)}
  '';
}
