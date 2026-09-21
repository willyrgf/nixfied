# Metadata checks only. Nixpkgs still owns option evaluation and value checking.
{ lib }:
let
  nonBlank = value: builtins.isString value && builtins.match "[[:space:]]*" value == null;
  nativeType =
    type:
    lib.types.isOptionType type
    && nonBlank (type.description or null)
    && builtins.all (name: lib.isFunction (type.${name} or null)) [
      "check"
      "merge"
      "getSubOptions"
    ];
  mkOption =
    attrs:
    let
      unknown = builtins.removeAttrs attrs [
        "type"
        "description"
        "default"
        "defaultText"
        "example"
        "apply"
      ];
    in
    assert lib.assertMsg (unknown == { }) "Nixfied option: unknown metadata attributes";
    assert lib.assertMsg (
      attrs ? type && nativeType attrs.type
    ) "Nixfied option: a native option type is required";
    assert lib.assertMsg (
      attrs ? description && nonBlank attrs.description
    ) "Nixfied option: a nonblank description is required";
    lib.mkOption attrs;

  # Audit before native rendering can hide a parent and its descendants.
  # Inspect metadata only; defaults/examples and configured values stay lazy.
  audit =
    path: options:
    builtins.all (
      name:
      let
        option = options.${name};
        loc = path ++ [ name ];
        label = lib.concatStringsSep "." loc;
      in
      if name == "_module" then
        true
      else if lib.isOption option then
        assert lib.assertMsg (
          (option.visible or true) == true && !(option.internal or false)
        ) "Nixfied option ${label}: framework options cannot hide from the reference";
        assert lib.assertMsg (nonBlank (
          option.description or null
        )) "Nixfied option ${label}: a nonblank description is required";
        assert lib.assertMsg (nativeType (
          option.type or null
        )) "Nixfied option ${label}: a native option type is required";
        audit option.loc (option.type.getSubOptions option.loc)
      else
        audit loc option
    ) (builtins.attrNames options);

  collect =
    options:
    let
      entries = builtins.filter (entry: !(builtins.elem "_module" entry.loc)) (
        lib.optionAttrSetToDocList (builtins.removeAttrs options [ "_module" ])
      );
    in
    builtins.seq (audit [ ] options) entries;
in
{
  inherit mkOption collect;
}
