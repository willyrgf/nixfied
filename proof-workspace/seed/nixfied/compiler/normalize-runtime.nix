{ lib }:
{
  resolved,
  statePolicy,
}:
let
  envNames =
    if resolved.runtime.env.names == [ ] then
      builtins.sort builtins.lessThan (builtins.attrNames resolved.runtime.env.offsets)
    else
      resolved.runtime.env.names;

  envOffsets = lib.genAttrs envNames (name: resolved.runtime.env.offsets.${name} or 0);
in
{
  slot = {
    inherit (resolved.runtime.slot) var;
    inherit (resolved.runtime.slot) default;
    inherit (resolved.runtime.slot) max;
    inherit (resolved.runtime.slot) stride;
  };

  env = {
    inherit (resolved.runtime.env) var;
    names = envNames;
    offsets = envOffsets;
    inherit (resolved.runtime.env) default;
  };

  logging = {
    inherit (resolved.runtime.logging) levelDefault;
    inherit (resolved.runtime.logging) outputDefault;
  };

  orchestrator = {
    inherit (resolved.runtime.orchestrator) stopTimeoutSec;
  };

  primitives = {
    inherit (resolved.runtime.primitives) version;
    inherit (resolved.runtime.primitives) defs;
  };

  inherit (resolved.runtime) ports;

  directories = {
    base = statePolicy.runtimeBase;
  };

  ephemeral = {
    inherit (resolved.runtime.ephemeral) copyMode;
    inherit (resolved.runtime.ephemeral) includeUntracked;
    inherit (resolved.runtime.ephemeral) excludePatterns;
    inherit (resolved.runtime.ephemeral) extraDirs;
    inherit (resolved.runtime.ephemeral) keepFailures;
    inherit (resolved.runtime.ephemeral) maxFailedRoots;
    inherit (resolved.runtime.ephemeral) maxFailedRootAgeHours;
    inherit (resolved.runtime.ephemeral) maxCopyBytes;
    inherit (resolved.runtime.ephemeral) minFreeBytesAfterCopy;
    inherit (resolved.runtime.ephemeral) envFileMode;
    inherit (resolved.runtime.ephemeral) envFilePath;
  };

  runtimePackages = map builtins.toString (resolved.tooling.runtimePackages or [ ]);
}
