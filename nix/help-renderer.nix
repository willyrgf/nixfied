apps:
let
  names = builtins.attrNames apps;
  nameWidth = builtins.foldl' (
    width: name:
    let
      length = builtins.stringLength name;
    in
    if length > width then length else width
  ) 0 names;
  padding =
    name:
    builtins.concatStringsSep "" (builtins.genList (_: " ") (nameWidth - builtins.stringLength name));
  render =
    name:
    let
      app = apps.${name};
      description = app.meta.description or (throw "app '${name}' has no meta.description");
      validDescription =
        if !builtins.isString description || description == "" then
          throw "app '${name}' has an empty or non-string meta.description"
        else
          true;
    in
    builtins.seq validDescription "  ${name}${padding name}  ${description}\n";
in
"Nixfied-generated commands:\n\n" + builtins.concatStringsSep "" (map render names)
