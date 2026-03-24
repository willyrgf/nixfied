{
  lib,
  canonical,
}:
let
  types = import ./types.nix {
    inherit
      lib
      canonical
      ;
  };
in
{
  inherit types;

  renderJsonSchema = import ./render-json-schema.nix {
    inherit
      lib
      canonical
      types
      ;
  };

  renderDocs = import ./render-docs.nix {
    inherit
      lib
      types
      ;
  };
}
