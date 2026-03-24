{
  lib,
  canonical,
}: {
  contractBundle,
}:
let
  sortNames = attrs: builtins.sort builtins.lessThan (builtins.attrNames attrs);

  bundle = contractBundle.bundle or (throw "nixfied compile-validation-ir: missing contract bundle");
  definitions = bundle.definitions or { };
  definitionNames = sortNames (builtins.attrNames definitions);
  schemaDocuments = contractBundle.jsonSchemaDocuments or { };
  validationSchemas = contractBundle.validationSchemas or { };

  irValidationSchemas = builtins.listToAttrs (
    map (
      name: {
        inherit name;
        value = if schemaDocuments ? name then schemaDocuments.${name} else validationSchemas.${name};
      }
    ) definitionNames
  );
in
canonical.canonicalize {
  kind = "nixfied-validation-ir";
  version = 1;
  definitionNames = definitionNames;
  definitions = definitions;
  validationSchemas = irValidationSchemas;
  docs = if (contractBundle ? docs) then contractBundle.docs else "";
}

