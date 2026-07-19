{ model }:

let
  inherit (builtins) attrNames;
  # Framework-owned public surfaces: the generated view/runtime command set every
  # model exposes. A view concern, derived here rather than mirrored in the model.
  surfaceNames = [
    "model"
    "schema"
    "docs"
    "capabilities"
    "check"
    "run"
    "ps"
    "down"
    "clean"
  ];
  serviceNames = attrNames model.services;
  taskNames = attrNames model.tasks;
  slots = builtins.genList (i: model.slotPolicy.min + i) (
    model.slotPolicy.max - model.slotPolicy.min + 1
  );
in
{
  schema = {
    schemaVersion = 1;
    source = "model.json";
    surfaces = surfaceNames;
    modelTypes = {
      modelVersion = model.modelVersion;
      runtimeAbi = model.runtimeAbi;
      toolchainId = model.toolchainId;
      primitives = [
        "Invocation"
        "Endpoint"
        "ProbeSpec"
        "Lifecycle"
        "TerminalSemantics"
        "ServiceSpec"
        "TaskSpec"
        "SecretDescriptor"
        "SecretSource"
        "SlotPlacement"
      ];
    };
  };

  # The capabilities view is a projection of the model, derived on demand rather
  # than carried as a redundant model section.
  capabilities = {
    environments = model.environments;
    inherit slots;
    services = serviceNames;
    tasks = taskNames;
    surfaces = surfaceNames;
  };

  docs = ''
    # ${model.docs.title}

    ${model.docs.summary}

    ## Target

    - system: ${model.target.system}
    - runtime ABI: ${model.runtimeAbi}
    - toolchain: ${model.toolchainId}

    ## Surfaces

    ${builtins.concatStringsSep "\n" (map (name: "- ${name}") surfaceNames)}

    ## Services

    ${builtins.concatStringsSep "\n" (map (service: "- ${service}") serviceNames)}

    ## Lifecycle

    ${builtins.concatStringsSep "\n" (
      map (
        service:
        let
          serviceSpec = model.services.${service};
          classes = builtins.attrNames serviceSpec.lifecycle;
        in
        "- ${service}: endpoints ${
          builtins.concatStringsSep ", " (builtins.attrNames (serviceSpec.endpoints or { }))
        } (primary ${
          serviceSpec.primaryEndpoint or "none"
        }); operations ${builtins.concatStringsSep ", " classes}"
      ) serviceNames
    )}

    ## Tasks

    ${builtins.concatStringsSep "\n" (map (task: "- ${task}") taskNames)}
  '';
}
