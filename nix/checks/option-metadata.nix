# Pure evaluation proofs; expected values are independent of the declarations.
{
  lib,
  pkgs,
  system,
}:
let
  metadata = import ../meta/options.nix { inherit lib; };
  inherit (metadata) mkOption;
  rejects = value: !(builtins.tryEval (builtins.deepSeq value true)).success;
  evaluate =
    target: module:
    import ../compiler/resolve.nix {
      inherit lib pkgs module;
      system = target;
    };
  empty = evaluate system { };
  entries = metadata.collect empty.options;
  configured = evaluate system (
    { lib, ... }: {
      nixfied.tasks.one.invocation = {
        tools = [ "tool" ];
        run = [ "tool" ];
        timeoutMs = lib.mkForce 42;
      };
      nixfied.services.one.lifecycle.start.invocation = {
        tools = [ "tool" ];
        run = [ "tool" ];
      };
    }
  );
  find = path: lib.findFirst (entry: entry.loc == path) (throw "missing mounted option") entries;
  fixture =
    minimum:
    let
      domain = (lib.types.addCheck lib.types.ints.positive (value: value >= minimum)) // {
        description = "integer >= ${toString minimum}";
      };
      invocation = lib.types.submodule {
        options =
          (import ../modules/invocation.nix {
            inherit lib;
            positiveInt = domain;
          })
          // {
            note = mkOption {
              type = lib.types.str;
              default = "shared";
              description = "An ordinary fixture option added to the shared invocation fragment.";
            };
          };
      };
    in
    lib.evalModules {
      modules = [
        {
          options = {
            task = mkOption {
              type = invocation;
              description = "Task invocation.";
            };
            services = mkOption {
              type = lib.types.attrsOf (
                lib.types.submodule {
                  options = {
                    start = mkOption {
                      type = invocation;
                      description = "Service start invocation.";
                    };
                    probe = mkOption {
                      type = lib.types.nullOr invocation;
                      default = null;
                      description = "Nullable exec probe.";
                    };
                  };
                }
              );
              description = "Keyed services.";
            };
          };
        }
      ];
    };
  accepts =
    minimum: value:
    let
      configured = (fixture minimum).extendModules {
        modules = [
          {
            task.timeoutMs = value;
            services.one.start.timeoutMs = value;
            services.one.probe.timeoutMs = value;
          }
        ];
      };
    in
    !(rejects [
      configured.config.task.timeoutMs
      configured.config.services.one.start.timeoutMs
      configured.config.services.one.probe.timeoutMs
    ]);
  fixtureEntry =
    minimum: name:
    lib.findFirst (entry: entry.name == name) null (metadata.collect (fixture minimum).options);
  extended = (fixture 1).extendModules {
    modules = [
      { task.note = lib.mkDefault "defaulted"; }
      {
        task.note = lib.mkForce "overridden";
        services.one.start = { };
        services.one.probe.note = "probe override";
      }
    ];
  };
  lazyModule = {
    options = {
      contextual = mkOption {
        type = lib.types.str;
        description = "Contextual default.";
        default = throw "contextual default forced";
        defaultText = lib.literalExpression "nativeContext";
      };
      required = mkOption {
        type = lib.types.str;
        description = "Required input.";
      };
      empty = mkOption {
        type = lib.types.attrsOf lib.types.str;
        description = "Native empty value.";
      };
    };
  };
  lazyOptions = lib.evalModules { modules = [ lazyModule ]; };
  lazyEntries = metadata.collect lazyOptions.options;
  defaultOf = name: (lib.findFirst (entry: entry.name == name) null lazyEntries).default.text;
  parentEntries =
    attrs:
    metadata.collect
      (lib.evalModules {
        modules = [
          {
            options.parent = lib.mkOption (
              {
                description = "Nested metadata must be audited before rendering.";
                type = lib.types.submodule lazyModule;
              }
              // attrs
            );
          }
        ];
      }).options;
  mountedOptions =
    description:
    let
      nested = lib.types.submodule {
        options.undocumented = lib.mkOption (
          {
            type = lib.types.str;
            default = throw "mounted option default forced";
            defaultText = lib.literalExpression "nativeContext";
          }
          // description
        );
      };
    in
    (lib.evalModules {
      modules = [
        {
          options.items = mkOption {
            description = "Keyed native mount.";
            type = lib.types.attrsOf (
              lib.types.submodule {
                options.nested = mkOption {
                  description = "Prefix-sensitive native suboptions.";
                  type = lib.types.mkOptionType {
                    name = "mounted-prefix-fixture";
                    description = "Native fixture with prefix-dependent suboptions";
                    inherit (nested) check merge;
                    getSubOptions =
                      prefix:
                      if
                        prefix == [
                          "items"
                          "<name>"
                          "nested"
                        ]
                      then
                        nested.getSubOptions prefix
                      else
                        { };
                  };
                };
              }
            );
          };
        }
      ];
    }).options;
  poisoned = import ../compiler/resolve.nix {
    inherit lib;
    pkgs = throw "reference demanded packages";
    system = throw "reference demanded contextual default";
    module = { };
  };
  source = pkgs.writeText "option-context-proof" "source";
  transformed =
    (evaluate system {
      nixfied.codebases.main.sourceIdentity = source.outPath;
    }).config.nixfied.codebases.main.sourceIdentity;
in
assert builtins.elem [ "items" "<name>" "nested" "undocumented" ] (
  map (entry: entry.loc) (lib.optionAttrSetToDocList (mountedOptions { }))
);
assert rejects (builtins.length (metadata.collect (mountedOptions { })));
assert
  (lib.findFirst
    (
      entry:
      entry.loc == [
        "items"
        "<name>"
        "nested"
        "undocumented"
      ]
    )
    null
    (
      metadata.collect (mountedOptions {
        description = "Documented native descendant.";
      })
    )
  ).default.text == "nativeContext";
assert rejects (parentEntries {
  visible = false;
});
assert rejects (parentEntries {
  visible = "shallow";
});
assert rejects (parentEntries {
  internal = true;
});
assert builtins.length (parentEntries { }) == 4;
assert
  (lib.findFirst (entry: entry.name == "parent.contextual") null (parentEntries { })).default.text
  == "nativeContext";
assert builtins.length entries == 128;
assert rejects (mkOption {
  type = lib.types.str;
});
assert rejects (mkOption {
  description = "Missing type.";
});
assert rejects (mkOption {
  type = "string";
  description = "Wrong type.";
});
assert rejects (mkOption {
  type._type = "option-type";
  description = "Missing native interface.";
});
assert rejects (mkOption {
  type = lib.types.str;
  description = " \n\t";
});
assert rejects (mkOption {
  type = lib.types.str;
  description = "Valid.";
  typo = true;
});
assert rejects (
  metadata.collect
    (lib.evalModules {
      modules = [
        {
          options = {
            good = mkOption {
              type = lib.types.str;
              description = "Good.";
            };
            unrelated = lib.mkOption {
              type = lib.types.str;
              description = " ";
            };
          };
        }
      ];
    }).options
);
assert rejects (
  metadata.collect
    (lib.evalModules {
      modules = [
        {
          options.hidden = lib.mkOption {
            type = lib.types.str;
            description = "A manual metadata bypass.";
            internal = true;
          };
        }
      ];
    }).options
);
assert
  (mkOption {
    type = lib.types.str;
    description = "Lazy default.";
    default = throw "default forced by metadata";
  }).description == "Lazy default.";
assert rejects empty.config.nixfied.project.name;
assert (evaluate "aarch64-linux" { }).config.nixfied.target.system == "aarch64-linux";
assert (evaluate "x86_64-linux" { }).config.nixfied.target.system == "x86_64-linux";
assert
  (evaluate "aarch64-linux" { nixfied.target.system = "x86_64-linux"; }).config.nixfied.target.system
  == "x86_64-linux";
assert
  (find [
    "nixfied"
    "target"
    "system"
  ]).default.text == "system";
assert configured.config.nixfied.tasks.one.invocation.timeoutMs == 42;
assert configured.config.nixfied.services.one.lifecycle.start.invocation.timeoutMs == 30000;
assert
  (find [
    "nixfied"
    "services"
    "<name>"
    "lifecycle"
    "ready"
    "probe"
    "invocation"
    "timeoutMs"
  ]).default.text == "30000";
assert rejects
  (evaluate system { nixfied.tasks.bad.invocation.timeoutMs = 0; })
  .config.nixfied.tasks.bad.invocation.timeoutMs;
assert rejects
  (evaluate system { nixfied.tasks.bad.invocation.timeoutMs = "30"; })
  .config.nixfied.tasks.bad.invocation.timeoutMs;
assert
  (evaluate system { nixfied.codebases.main.sourceIdentity = "source"; })
  .config.nixfied.codebases.main.sourceIdentity == "source";
assert
  (evaluate system { nixfied.codebases.main.sourceIdentity = ../.; })
  .config.nixfied.codebases.main.sourceIdentity == toString ../.;
assert rejects
  (evaluate system { nixfied.codebases.main.sourceIdentity = 1; })
  .config.nixfied.codebases.main.sourceIdentity;
assert accepts 1 1;
assert !(accepts 2 1);
assert accepts 2 2;
assert (fixtureEntry 1 "task.timeoutMs").type == "integer >= 1";
assert (fixtureEntry 2 "task.timeoutMs").type == "integer >= 2";
assert (fixtureEntry 2 "services.<name>.start.timeoutMs").type == "integer >= 2";
assert (fixtureEntry 1 "task.note").default.text == ''"shared"'';
assert
  (fixtureEntry 1 "services.<name>.start.note").description
  == "An ordinary fixture option added to the shared invocation fragment.";
assert extended.config.task.note == "overridden";
assert extended.config.services.one.start.note == "shared";
assert extended.config.services.one.probe.note == "probe override";
assert (fixtureEntry 1 "services.<name>.probe.note").default.text == ''"shared"'';
assert (fixtureEntry 2 "services.<name>.probe.timeoutMs").type == "integer >= 2";
assert defaultOf "contextual" == "nativeContext";
assert defaultOf "empty" == "{ }";
assert !(lib.findFirst (entry: entry.name == "required") null lazyEntries ? default);
assert map (entry: entry.loc) (metadata.collect poisoned.options) == map (entry: entry.loc) entries;
assert builtins.hasContext transformed;
assert transformed == source.outPath;
true
