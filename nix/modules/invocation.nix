# Ordinary native option fragment, mounted by task/start/probe submodules.
{ lib, positiveInt }:
let
  inherit (lib) types;
  vocabulary = (import ../meta/default.nix { inherit lib; }).vocabularyMap;
  inherit (import ../meta/options.nix { inherit lib; }) mkOption;
in
{
  tools = mkOption {
    type = types.nonEmptyListOf (types.either types.nonEmptyStr types.package);
    description = "Tool set: declared closure ids or packages; their bin roots form the child PATH in order.";
  };
  run = mkOption {
    type = types.nonEmptyListOf types.str;
    description = "Argv. run[0] must be the executable basename of one tool.";
  };
  env = mkOption {
    type = types.attrsOf types.str;
    default = { };
    description = "Declared child environment (hermetic: nothing else is inherited; PATH is runtime-owned).";
  };
  codebaseId = mkOption {
    type = types.nonEmptyStr;
    default = "main";
    description = "Codebase the invocation observes.";
  };
  cwd = mkOption {
    type = types.nonEmptyStr;
    default = ".";
    description = "Confined relative working directory under the codebase.";
  };
  stdin = mkOption {
    type = types.enum vocabulary."enum StdinPolicy".members;
    default = "null";
    description = "Whether the child receives closed stdin (`null`) or the runtime command's stdin (`inherit`).";
  };
  timeoutMs = mkOption {
    type = positiveInt;
    default = 30000;
    description = "Maximum invocation duration in milliseconds before cancellation.";
  };
}
