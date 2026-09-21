# Independent rejection/laziness vectors for the publication boundary.
{
  lib,
  pkgs,
  system,
}:
let
  assemble =
    declarations:
    import ../meta/publications.nix { inherit lib; } {
      inherit declarations;
      targets = [
        {
          kind = "topic";
          id = "native";
        }
        {
          kind = "option";
          path = [
            "nixfied"
            "tasks"
            "<name>"
            "invocation"
            "run"
          ];
        }
      ];
    };
  rejects = value: !(builtins.tryEval (builtins.deepSeq value true)).success;
  function = {
    kind = "function";
    scope = "library";
    name = "identity";
    description = "Return the input.";
    usage = "identity value";
    input = "Any native value.";
    result = "The same value.";
    binding = value: value;
  };
  app = {
    kind = "app";
    scope = "root";
    name = "inspect";
    description = "Inspect static content.";
    usage = "nix run .#inspect";
    effects = "Reads packaged content.";
    topic = "native";
    binding = {
      type = "app";
      program = "/native/inspect";
    };
  };
  package = {
    kind = "package";
    scope = "check";
    name = "audit";
    description = "Audit declarations.";
    usage = "nix build .#checks.<system>.audit";
    artifact = "Successful audit result.";
    binding = throw "metadata forced an audit derivation";
  };
  checked = assemble [
    function
    app
    package
    (app // { scope = "project"; })
  ];
  poisoned = import ../meta/authoring.nix {
    inherit lib;
    pkgs = throw "bootstrap forced pkgs";
    system = throw "bootstrap forced system";
  };
  resolved = import ../compiler/resolve.nix {
    inherit lib;
    pkgs = throw "publication observation forced pkgs";
    system = throw "publication observation forced system";
    module = { };
  };
  actualArgs = resolved._module.specialArgs;
  actualAdapters = actualArgs.adapters;
  sentinel = pkgs.runCommand "nixfied-docs-unused-sentinel" { } "exit 1";
  docs = import ../docs/reference.nix {
    inherit lib pkgs system;
    options = poisoned.options;
    publications =
      (assemble [
        (function // { description = "Display ${sentinel}"; })
        package
      ]).entries;
    source.path = "${sentinel}/source";
  };
in
assert (checked.project "function" "library").identity 42 == 42;
assert builtins.length checked.entries == 4;
assert rejects
  (
    (assemble [
      function
      (app // { description = " "; })
    ]).project
    "function"
    "library"
  ).identity;

assert checked.names "app" "project" == [ "inspect" ];
assert (checked.project "app" "root").inspect.meta.description == "Inspect static content.";
assert rejects
  (assemble [
    function
    function
  ]).entries;
assert rejects
  (assemble [
    function
    (app // { description = " \n"; })
  ]).entries;
assert rejects (assemble [ (function // { typo = true; }) ]).entries;
assert rejects (assemble [ (function // { effects = "Not a function field."; }) ]).entries;
assert rejects (assemble [ (function // { scope = "root"; }) ]).entries;
assert rejects (assemble [ (builtins.removeAttrs function [ "binding" ]) ]).entries;
assert rejects (assemble [ (app // { command = "future"; }) ]).entries;
assert rejects (assemble [ (builtins.removeAttrs app [ "topic" ]) ]).entries;
assert rejects (assemble [ (app // { topic = "missing"; }) ]).entries;
assert rejects
  (assemble [
    (
      function
      // {
        references = [
          {
            kind = "app";
            id = "library/identity";
          }
        ];
      }
    )
  ]).entries;
assert rejects
  ((assemble [ (function // { binding = 1; }) ]).project "function" "library").identity;
assert rejects
  (
    (assemble [
      (
        app
        // {
          binding = {
            type = "app";
            program = 1;
          };
        }
      )
    ]).project
    "app"
    "root"
  ).inspect;
assert
  builtins.length
    (assemble [
      (
        function
        // {
          references = [
            {
              kind = "option";
              path = [
                "nixfied"
                "tasks"
                "<name>"
                "invocation"
                "run"
              ];
            }
          ];
        }
      )
    ]).entries == 1;
assert rejects
  (assemble [
    (
      function
      // {
        references = [
          {
            kind = "option";
            id = "nixfied.tasks.<name>.invocation.run";
          }
        ];
      }
    )
  ]).entries;
assert rejects
  (assemble [
    (
      function
      // {
        references = [
          {
            kind = "option";
            path = [
              "nixfied"
              "tasks"
              "<name>"
              "invocation"
              "missing"
            ];
          }
        ];
      }
    )
  ]).entries;
assert checked.audit "app" "root" { inspect = throw "audit forced binding"; };
assert rejects (
  checked.audit "app" "root" {
    inspect = null;
    bypass = null;
  }
);
assert rejects (checked.audit "app" "root" { });
assert builtins.length poisoned.publication.entries == 7;
assert poisoned.publication.audit "argument" "module-argument" actualArgs;
assert poisoned.publication.audit "module" "adapter" actualAdapters;
assert rejects (
  poisoned.publication.audit "argument" "module-argument" (actualArgs // { undocumented = null; })
);
assert rejects (
  poisoned.publication.audit "argument" "module-argument" (builtins.removeAttrs actualArgs [ "pkgs" ])
);
assert rejects (
  poisoned.publication.audit "module" "adapter" (actualAdapters // { undocumented = null; })
);
assert rejects (
  poisoned.publication.audit "module" "adapter" (builtins.removeAttrs actualAdapters [ "postgres" ])
);
assert builtins.length poisoned.options == 128;
assert builtins.getContext docs.serialized == { };
assert builtins.hasContext "${sentinel}/bin/native";
true
