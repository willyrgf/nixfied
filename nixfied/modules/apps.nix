{ lib, ... }:
let
  t = lib.types;
in
{
  options.nixfied.apps = lib.mkOption {
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
              type = t.enum [ "taskRef" ];
              default = "taskRef";
            };

            taskId = lib.mkOption {
              type = t.str;
              default = "";
            };

            summary = lib.mkOption {
              type = t.str;
              default = "";
            };

            description = lib.mkOption {
              type = t.str;
              default = "";
            };

            category = lib.mkOption {
              type = t.str;
              default = "core";
            };

            usage = lib.mkOption {
              type = t.listOf t.str;
              default = [ ];
            };

            examples = lib.mkOption {
              type = t.listOf t.str;
              default = [ ];
            };

            ownerFile = lib.mkOption {
              type = t.nullOr t.str;
              default = null;
            };
          };
        }
      )
    );
    default = { };
  };
}
