{ lib }:
let
  t = lib.types;
in
{
  mkLauncherOptions =
    {
      defaultAppId,
      defaultCategory ? "core",
      defaultOwnerFile ? null,
    }:
    {
      enable = lib.mkOption {
        type = t.bool;
        default = false;
      };

      appId = lib.mkOption {
        type = t.str;
        default = defaultAppId;
      };

      summary = lib.mkOption {
        type = t.str;
        default = "";
      };

      description = lib.mkOption {
        type = t.str;
        default = "";
      };

      usage = lib.mkOption {
        type = t.listOf t.str;
        default = [ ];
      };

      examples = lib.mkOption {
        type = t.listOf t.str;
        default = [ ];
      };

      category = lib.mkOption {
        type = t.str;
        default = defaultCategory;
      };

      ownerFile = lib.mkOption {
        type = t.nullOr t.str;
        default = defaultOwnerFile;
      };
    };
}
