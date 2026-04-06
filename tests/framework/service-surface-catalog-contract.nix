{ pkgs }:
let
  inherit (pkgs) lib;
  frameworkLib = import ../../nixfied/framework/core {
    inherit pkgs;
    inherit (pkgs) system;
  };
  compiled = frameworkLib.mkNixfied {
    projectRoot = ../..;
    projectModules = [ ../../nixfied/project/module.nix ];
    extraModules = [ ];
    localOverrides = [ ];
  };
  catalog = compiled.model.compiled.serviceSurfaceCatalog;
  sortKeys = attrs: builtins.sort builtins.lessThan (builtins.attrNames attrs);
  serviceNames = sortKeys (catalog.serviceApis or { });
  heavyServiceAppNames = builtins.sort builtins.lessThan (
    builtins.filter (appName: lib.hasPrefix "svc::" appName) (
      builtins.attrNames (compiled.model.views.apps or { })
    )
  );
  operationEntries = builtins.concatLists (
    map (
      serviceName:
      map (opName: catalog.operationCatalog.${serviceName}.${opName}) (
        sortKeys (catalog.operationCatalog.${serviceName} or { })
      )
    ) serviceNames
  );
  publicCatalogJson = builtins.toJSON (catalog.serviceApis or { });
  publicHeavyJson = builtins.toJSON compiled.serviceApis;
in
assert catalog.appNames == heavyServiceAppNames;
assert serviceNames == sortKeys compiled.serviceApis;
assert !(lib.hasInfix "\"adapter\"" publicCatalogJson);
assert !(lib.hasInfix "\"implementation\"" publicCatalogJson);
assert !(lib.hasInfix "\"adapter\"" publicHeavyJson);
assert !(lib.hasInfix "\"implementation\"" publicHeavyJson);
assert !(lib.hasInfix "framework/runtime/services/" publicCatalogJson);
assert !(lib.hasInfix "modules/services/runtime/" publicCatalogJson);
assert !(lib.hasInfix "framework/runtime/services/" publicHeavyJson);
assert !(lib.hasInfix "modules/services/runtime/" publicHeavyJson);
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
  && !(catalogApi ? adapter)
  && !(catalogApi ? implementation)
  && !(heavyApi ? adapter)
  && !(heavyApi ? implementation)
) serviceNames;
assert lib.all (
  serviceName:
  catalog.serviceApis.${serviceName}.ownerFile == "nixfied/modules/services/${serviceName}.nix"
) serviceNames;
assert lib.all (
  entry:
  let
    viewApp = (compiled.model.views.apps or { }).${entry.appName} or null;
    catalogApp = (catalog.appsByName or { }).${entry.appName} or null;
  in
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
      && catalogApp.commandApi == entry.commandApi
    )
  )
) operationEntries;
assert lib.all (entry: !(entry ? hookName) && !(entry ? includeHook)) operationEntries;
assert lib.all (entry: (entry.commandApi.version or null) == 2) operationEntries;
pkgs.runCommand "service-surface-catalog-contract" { } ''
  echo "OK: compiled service surface catalog remains the sole public authority for service APIs and svc app descriptors" > "$out"
''
