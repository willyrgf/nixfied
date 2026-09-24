# Compile distinguishable same-name outputs; expectations do not derive IDs.
{ lib, pkgs }:
let
  system = pkgs.stdenv.hostPlatform.system;
  package =
    value:
    pkgs.writeShellScriptBin "identity-tool" ''
      printf '%s\n' '${value}'
    '';
  first = package "first";
  second = package "second";
  alternate = first // {
    meta = first.meta // {
      mainProgram = "alternate";
    };
  };
  compile =
    extra:
    let
      evaluated = import ../compiler/resolve.nix {
        inherit lib pkgs system;
        module = {
          nixfied = {
            project = {
              projectId = "identity";
              name = "identity";
            };
            tasks = {
              first.invocation = {
                tools = [ first ];
                run = [ "identity-tool" ];
              };
              again.invocation = {
                tools = [ first ];
                run = [ "identity-tool" ];
              };
              second.invocation = {
                tools = [ second ];
                run = [ "identity-tool" ];
              };
              alternate.invocation = {
                tools = [ alternate ];
                run = [ "alternate" ];
              };
            };
          };
          imports = [ extra ];
        };
      };
    in
    (import ../compiler/derive.nix {
      inherit lib pkgs system;
      constants = import ../spec/constants.nix;
      config = import ../compiler/validate.nix { inherit lib system; } evaluated.config;
    }).manifest;
  manifest = compile { };
  invocation = name: manifest.tasks.${name}.invocation;
  id = name: builtins.head (invocation name).tools;
  collision = compile {
    nixfied.closures.${id "first"} = {
      package = second;
      executable = "bin/identity-tool";
    };
  };
in
assert id "first" == id "again";
assert id "first" != id "second";
assert id "first" != id "alternate";
assert builtins.length (builtins.attrNames manifest.closures) == 3;
assert (invocation "first").executable == "${first}/bin/identity-tool";
assert (invocation "second").executable == "${second}/bin/identity-tool";
assert (invocation "alternate").executable == "${first}/bin/alternate";
assert
  manifest.closures.${id "first"}.operationBindings == [
    "task.again.run"
    "task.first.run"
  ];
assert !(builtins.tryEval (builtins.deepSeq collision true)).success;
pkgs.runCommand "tool-identity" { } ''
  test "$(${lib.escapeShellArg (invocation "first").executable})" = first
  test "$(${lib.escapeShellArg (invocation "second").executable})" = second
  test "$(PATH=${
    lib.escapeShellArg (builtins.dirOf manifest.closures.${id "first"}.executable)
  } identity-tool)" = first
  test "$(PATH=${
    lib.escapeShellArg (builtins.dirOf manifest.closures.${id "second"}.executable)
  } identity-tool)" = second
  touch "$out"
''
