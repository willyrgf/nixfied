{ pkgs }:
let
  validation = import ./validation.nix { inherit pkgs; };
  inherit (validation)
    expect
    renderErrors
    sortedAttrNames
    ;

  isScriptLike = x: (builtins.isString x) || (builtins.isPath x) || (builtins.isAttrs x);

  validateServiceImplementationMatchesCatalogErrors =
    {
      serviceName,
      serviceApi,
      implementation,
    }:
    let
      ops = serviceApi.operations or { };
      implementationOps = implementation.operations or { };
      implementationOpErrors = builtins.concatLists (
        map (
          opName:
          let
            opCfg = ops.${opName};
            runtimeOp = opCfg.runtimeOp or opName;
          in
          if runtimeOp == null || runtimeOp == "" then
            [ ]
          else
            expect (builtins.hasAttr runtimeOp implementationOps) "${serviceName}.${opName}: runtime implementation is missing operation '${runtimeOp}'"
            ++
              expect (isScriptLike (implementationOps.${runtimeOp} or null))
                "${serviceName}.${opName}: runtime implementation operation '${runtimeOp}' must be string/path/derivation"
        ) (builtins.attrNames ops)
      );
    in
    if implementation == null then
      [ "${serviceName}: runtime implementation is required" ]
    else if !builtins.isAttrs implementation then
      [ "${serviceName}: runtime implementation must be an attribute set" ]
    else
      expect (
        (implementation.version or null) == 1
      ) "${serviceName}: runtime implementation version must be 1"
      ++ expect (
        implementation ? operations
      ) "${serviceName}: runtime implementation operations are required"
      ++ expect (builtins.isAttrs implementationOps) "${serviceName}: runtime implementation operations must be an attribute set"
      ++ implementationOpErrors;

  validateServiceImplementationsAgainstCatalog =
    {
      serviceApis,
      serviceImplementations,
    }:
    let
      names = sortedAttrNames serviceApis;
      errs = builtins.concatLists (
        map (
          serviceName:
          validateServiceImplementationMatchesCatalogErrors {
            inherit serviceName;
            serviceApi = serviceApis.${serviceName};
            implementation = serviceImplementations.${serviceName} or null;
          }
        ) names
      );
    in
    if errs == [ ] then
      serviceImplementations
    else
      throw ''
        Nixfied service runtime implementation violated:
        ${renderErrors errs}
      '';
in
{
  inherit
    sortedAttrNames
    validateServiceImplementationsAgainstCatalog
    ;
}
