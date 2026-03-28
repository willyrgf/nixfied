{ pkgs }:
let
  lib = pkgs.lib;
  frameworkLib = import ../../nixfied/framework/core {
    inherit pkgs;
    system = pkgs.system;
  };
  compiled = frameworkLib.mkNixfied {
    projectRoot = ../..;
    projectModules = [ ../../nixfied/project/module.nix ];
    extraModules = [ ];
    localOverrides = [ ];
  };
  catalog = compiled.model.compiled.serviceSurfaceCatalog;
  sortKeys = attrs: builtins.sort builtins.lessThan (builtins.attrNames attrs);
  heavyServiceAppNames = builtins.sort builtins.lessThan (
    builtins.filter (appName: lib.hasPrefix "svc::" appName) (
      builtins.attrNames (compiled.model.views.apps or { })
    )
  );
  mkServiceRuntimeSurfacesSource = builtins.readFile ../../nixfied/framework/core/mkServiceRuntimeSurfaces.nix;
  compileServiceSurfaceCatalogSource = builtins.readFile ../../nixfied/compiler/compile-service-surface-catalog.nix;
  serviceNames = sortKeys (catalog.serviceApis or { });
  operationEntries = builtins.concatLists (
    map (
      serviceName:
      map (opName: catalog.operationCatalog.${serviceName}.${opName}) (
        sortKeys (catalog.operationCatalog.${serviceName} or { })
      )
    ) serviceNames
  );
  mkCompiledCoreSource = builtins.readFile ../../nixfied/framework/core/mkCompiledCore.nix;
in
assert catalog.appNames == heavyServiceAppNames;
assert serviceNames == sortKeys compiled.serviceApis;
assert lib.all (
  serviceName:
  let
    catalogApi = catalog.serviceApis.${serviceName};
    heavyApi = compiled.serviceApis.${serviceName};
  in
  catalogApi.summary == heavyApi.summary
  && catalogApi.details == heavyApi.details
  && catalogApi.artifacts == heavyApi.artifacts
  && catalogApi.profiles == heavyApi.profiles
  && catalogApi.runtimePrimitives == heavyApi.runtimePrimitives
  && sortKeys (catalogApi.operations or { }) == sortKeys (heavyApi.operations or { })
) serviceNames;
assert lib.all (
  entry:
  let
    viewApp = (compiled.model.views.apps or { }).${entry.appName} or null;
    catalogApp = (catalog.appsByName or { }).${entry.appName} or null;
  in
  (builtins.hasAttr entry.hookName compiled.serviceHookEnv) == (entry.includeHook or false)
  &&
    (builtins.hasAttr entry.appName (compiled.model.views.apps or { })) == (entry.includeApp or false)
  && (
    !(entry.includeApp or false)
    || (
      viewApp != null
      && catalogApp != null
      && viewApp.kind == "service"
      && viewApp.summary == catalogApp.summary
      && viewApp.description == catalogApp.description
      && viewApp.category == catalogApp.category
      && viewApp.usage == catalogApp.usage
      && viewApp.examples == catalogApp.examples
      && viewApp.ownerFile == catalogApp.ownerFile
      && catalogApp.service == entry.serviceName
      && catalogApp.operation == entry.opName
      && catalog.appServiceByName.${entry.appName} == entry.serviceName
    )
  )
) operationEntries;
assert lib.all (
  serviceName:
  catalog.serviceApis.${serviceName}.ownerFile == "nixfied/modules/services/${serviceName}.nix"
) serviceNames;
assert !(lib.hasInfix "mkServiceSurfaceCatalog.nix" mkCompiledCoreSource);
assert
  !(lib.hasInfix "serviceModulePath = import ./serviceModulePath.nix;" mkServiceRuntimeSurfacesSource);
assert !(lib.hasInfix "mkServiceApisFromModules (" mkServiceRuntimeSurfacesSource);
assert lib.hasInfix "require compiled serviceSurfaceCatalog" mkServiceRuntimeSurfacesSource;
assert !(lib.hasInfix "serviceModulePath" compileServiceSurfaceCatalogSource);
assert !(lib.hasInfix "mkServiceApisFromModules" compileServiceSurfaceCatalogSource);
assert !(lib.hasInfix "publicApi" compileServiceSurfaceCatalogSource);
assert lib.hasInfix "serviceDefinitions" compileServiceSurfaceCatalogSource;
pkgs.runCommand "service-surface-catalog-contract" { } ''
  echo "OK: compiled service surface catalog is the only service API source for materialized service apps, hooks, and public descriptors" > "$out"
''
