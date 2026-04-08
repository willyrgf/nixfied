{ lib, config, ... }:
let
  t = lib.types;
  apiOptions = import ./lib/api-options.nix { inherit lib; };
  launcherOptions = import ./lib/launcher-options.nix { inherit lib; };
  serviceRequirementType = t.str;
  configuredServiceNames = builtins.sort builtins.lessThan (
    builtins.attrNames (config.nixfied.services or { })
  );
  validateServiceNames =
    context: names:
    let
      unknown = builtins.filter (name: !(builtins.elem name configuredServiceNames)) (
        builtins.sort builtins.lessThan names
      );
    in
    if unknown == [ ] then
      names
    else
      throw ''
        ${context} references unknown services: ${builtins.concatStringsSep ", " unknown}
        known services: ${builtins.concatStringsSep ", " configuredServiceNames}
      '';
  runtimeWorkdirType = t.enum [
    "projectRoot"
    "stateRoot"
    "custom"
  ];

  hookSpec = t.submodule {
    options = {
      command = lib.mkOption { type = t.lines; };
      runtimeInputs = lib.mkOption {
        type = t.listOf t.package;
        default = [ ];
      };
      passThroughEnv = lib.mkOption {
        type = t.listOf t.str;
        default = [ ];
      };
      env = lib.mkOption {
        type = t.attrsOf (
          t.oneOf [
            t.str
            t.int
            t.bool
          ]
        );
        default = { };
      };
      workdir = lib.mkOption {
        type = t.nullOr runtimeWorkdirType;
        default = null;
      };
      customWorkdir = lib.mkOption {
        type = t.nullOr t.str;
        default = null;
      };
    };
  };

in
{
  options.nixfied.tasks = lib.mkOption {
    type = t.attrsOf (
      t.submodule (
        { name, ... }:
        {
          options = {
            id = lib.mkOption {
              type = t.str;
              default = name;
            };
            kind = lib.mkOption {
              type = t.enum [
                "command"
                "service-op"
                "supervisor-op"
                "utility"
                "ci-step"
                "workflow"
                "internal"
              ];
              default = "command";
            };
            summary = lib.mkOption {
              type = t.str;
              default = name;
            };
            description = lib.mkOption {
              type = t.str;
              default = "";
            };
            tags = lib.mkOption {
              type = t.listOf t.str;
              default = [ ];
            };

            requirements = {
              services = lib.mkOption {
                type = t.listOf serviceRequirementType;
                default = [ ];
                apply = validateServiceNames "nixfied.tasks.${name}.requirements.services";
                description = "Hard service capability requirements used for graph exclusion and runtime skip.";
              };
            };

            runner = {
              type = lib.mkOption {
                type = t.enum [
                  "shell"
                  "derivation"
                  "workflowRef"
                ];
                default = "shell";
              };
              command = lib.mkOption {
                type = t.lines;
                default = "";
              };
              package = lib.mkOption {
                type = t.nullOr t.package;
                default = null;
              };
              workflowId = lib.mkOption {
                type = t.nullOr t.str;
                default = null;
              };
            };

            commandApi = lib.mkOption {
              type = apiOptions.commandApi;
              default = { };
              description = "Canonical command API metadata for the task.";
            };

            launcher = lib.mkOption {
              type = t.submodule {
                options = launcherOptions.mkLauncherOptions {
                  defaultAppId = name;
                };
              };
              default = { };
              description = "Compiled launcher metadata for exposing this task as a public app.";
            };

            runtime = {
              slotEnv = lib.mkOption {
                type = t.enum [
                  "required"
                  "optional"
                  "disabled"
                ];
                default = "optional";
              };
              workdir = lib.mkOption {
                type = runtimeWorkdirType;
                default = "projectRoot";
              };
              customWorkdir = lib.mkOption {
                type = t.nullOr t.str;
                default = null;
              };
              hermetic = lib.mkOption {
                type = t.bool;
                default = true;
              };
              runtimeInputs = lib.mkOption {
                type = t.listOf t.package;
                default = [ ];
              };
              passThroughEnv = lib.mkOption {
                type = t.listOf t.str;
                default = [ ];
              };
              references = {
                taskIds = lib.mkOption {
                  type = t.listOf t.str;
                  default = [ ];
                  description = "Additional task ids invoked through the runtime engine at execution time.";
                };
                workflowIds = lib.mkOption {
                  type = t.listOf t.str;
                  default = [ ];
                  description = "Additional workflow ids invoked through the runtime engine at execution time.";
                };
              };
              allowSensitivePassThrough = lib.mkOption {
                type = t.bool;
                default = false;
              };
              logging = {
                levelDefault = lib.mkOption {
                  type = t.nullOr (
                    t.enum [
                      "error"
                      "warn"
                      "info"
                      "debug"
                      "trace"
                    ]
                  );
                  default = null;
                };
                outputDefault = lib.mkOption {
                  type = t.nullOr (
                    t.enum [
                      "stdout"
                      "logs"
                      "both"
                    ]
                  );
                  default = null;
                };
              };
              env = lib.mkOption {
                type = t.attrsOf (
                  t.oneOf [
                    t.str
                    t.int
                    t.bool
                  ]
                );
                default = { };
              };
              umask = lib.mkOption {
                type = t.str;
                default = "022";
              };
              locale = lib.mkOption {
                type = t.str;
                default = "C.UTF-8";
              };
              timezone = lib.mkOption {
                type = t.str;
                default = "UTC";
              };
              preHooks = lib.mkOption {
                type = t.attrsOf hookSpec;
                default = { };
              };
              postHooks = lib.mkOption {
                type = t.attrsOf hookSpec;
                default = { };
              };
            };

            scheduling = {
              locks = lib.mkOption {
                type = t.listOf t.str;
                default = [ ];
              };
              maxAttempts = lib.mkOption {
                type = t.int;
                default = 1;
              };
              retryBackoffSec = lib.mkOption {
                type = t.listOf t.int;
                default = [ ];
              };
              priority = lib.mkOption {
                type = t.int;
                default = 100;
              };
            };

            deps = {
              needs = lib.mkOption {
                type = t.listOf t.str;
                default = [ ];
              };
              softNeeds = lib.mkOption {
                type = t.listOf t.str;
                default = [ ];
              };
            };

            produces = {
              artifacts = lib.mkOption {
                type = t.listOf t.str;
                default = [ ];
              };
              stateKeys = lib.mkOption {
                type = t.listOf t.str;
                default = [ ];
              };
            };
          };
        }
      )
    );
    default = { };
  };
}
