{ pkgs }:
let
  inherit (pkgs) lib;
  exitCodes = import ./exit-codes.nix;
  runtimePrimitives = import ./runtime-primitives.nix { };

  runtimeEnvSpecs = runtimePrimitives.mkRuntimePrimitiveEnvSpecs { };

  normalizeEnvSpec =
    spec:
    spec
    // {
      type = spec.type or "string";
      required = spec.required or false;
      aliases = spec.aliases or [ ];
    };

  mergeEnvSpecs =
    envSpecs:
    let
      existing = map (spec: spec.name) envSpecs;
      extras = lib.filter (spec: !(builtins.elem spec.name existing)) runtimeEnvSpecs;
    in
    map normalizeEnvSpec envSpecs ++ extras;

  mkCommandApi =
    {
      class ? "typed",
      name,
      summary,
      details ? "",
      usage ? [ ],
      examples ? [ ],
      args ? [ ],
      env ? [ ],
      category ? "core",
      idempotent ? false,
      allowUnknownArgs ? class == "passthrough",
      outputs ? null,
      failureCodes ? builtins.removeAttrs exitCodes [
        "canceled"
        "unavailable"
        "timeout"
      ],
    }:
    let
      resolvedOutputs =
        if outputs != null then
          outputs
        else if class == "json" then
          {
            mode = "json";
            keys = [ ];
          }
        else
          {
            mode = "text";
            keys = [ ];
          };
    in
    {
      version = 2;
      inherit
        summary
        details
        usage
        examples
        category
        ;
      commandApi = {
        version = 2;
        inherit
          name
          args
          allowUnknownArgs
          idempotent
          failureCodes
          ;
        commandClass = class;
        env = mergeEnvSpecs env;
        outputs = resolvedOutputs;
      };
    };
in
{
  inherit mkCommandApi;
}
