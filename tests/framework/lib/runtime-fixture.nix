{ pkgs }:
let
  frameworkLib = import ../../../nixfied/framework/core {
    inherit pkgs;
    system = pkgs.system;
  };
  lib = pkgs.lib;

  withCompiledExecution =
    model:
    let
      execution = frameworkLib.compileExecution {
        resolvedIdentity = model.identity or { };
        runtime = model.runtime or { };
        state = model.state or { };
        serviceCatalog = model.serviceCatalog or { };
        serviceSets = model.serviceSets or { };
        apps = model.apps or { };
        tasks = model.tasks or { };
        workflows = model.workflows or { };
      };
    in
    model
    // {
      compiled =
        (builtins.removeAttrs (model.compiled or { }) [
          "execution"
          "runtimeManifests"
        ])
        // {
          inherit execution;
        };
    };

  runtimeMaterialization =
    {
      model,
      services,
      pkgs ? null,
      serviceDefinitions ? null,
      resolvedServices ? null,
    }:
    let
      normalizedResolvedServices =
        if resolvedServices != null then
          resolvedServices
        else
          builtins.listToAttrs (
            map (
              serviceId:
              let
                service = services.${serviceId};
                serviceName =
                  if lib.hasPrefix "service." serviceId then
                    builtins.substring 8 ((builtins.stringLength serviceId) - 8) serviceId
                  else
                    service.name or serviceId;
              in
              {
                name = serviceName;
                value =
                  if service ? config then
                    (service.config or { })
                    // {
                      enable = service.enable or false;
                    }
                  else
                    service;
              }
            ) (builtins.attrNames services)
          );

      compiledCore = {
        inherit model;
        serviceSets = model.serviceSets or { };
        resolved = {
          services = if serviceDefinitions != null then serviceDefinitions else normalizedResolvedServices;
        };
        contractBundle = (model.compiled or { }).contractBundle or { };
      };

      materialized = frameworkLib.materializeExecution {
        projectRoot = ../../..;
        compiledCore = compiledCore;
      };
    in
    {
      inherit (materialized) services;
      serviceDispatcherProgram = "${materialized.serviceDispatcher}/bin/nixfied-service-dispatcher";
      runtimeEngineProgram = "${materialized.runtimeEngine}/bin/nixfied-runtime";
    };
in
{
  inherit
    withCompiledExecution
    runtimeMaterialization
    ;
}
