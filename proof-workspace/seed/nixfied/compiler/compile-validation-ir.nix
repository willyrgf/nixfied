{
  canonical,
  ...
}:
{
  contractBundle,
}:
let
  asAttrSet =
    value:
    if builtins.typeOf value == "set" then
      value
    else if builtins.typeOf value == "list" then
      builtins.listToAttrs value
    else
      throw "compile-validation-ir: expected set or canonicalized list";

  bundle = contractBundle.bundle or (throw "nixfied compile-validation-ir: missing contract bundle");
  definitions = asAttrSet (bundle.definitions or { });
  schemaDocuments = asAttrSet (contractBundle.jsonSchemaDocuments or { });
  validationSchemas = asAttrSet (contractBundle.validationSchemas or { });
  definitionNames = builtins.sort builtins.lessThan (builtins.attrNames definitions);

  irValidationSchemas = builtins.listToAttrs (
    map (name: {
      inherit name;
      value = if schemaDocuments ? name then schemaDocuments.${name} else validationSchemas.${name};
    }) definitionNames
  );
in
canonical.canonicalize {
  kind = "nixfied-validation-ir";
  version = 1;
  inherit definitionNames;
  inherit definitions;
  validationSchemas = irValidationSchemas;
  docs = if (contractBundle ? docs) then contractBundle.docs else "";
}
