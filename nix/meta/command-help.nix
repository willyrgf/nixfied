# Native presentation only: all visible arguments participate in either layout.
{ lib }:
let
  label =
    argument:
    argument.token + lib.optionalString (argument.help.metavar != null) " ${argument.help.metavar}";
  visible = builtins.filter (argument: argument.help.kind == "Visible");
in
{
  inherit label;
  choices =
    members:
    if builtins.length members < 2 then
      lib.concatStringsSep "" members
    else
      lib.concatStringsSep ", " (lib.init members) + ", or " + lib.last members;
  optionalArguments =
    arguments: lib.concatMapStrings (argument: " [${label argument}]") (visible arguments);
  runtimeRows =
    column: facts:
    let
      row =
        token: text:
        let
          lines = lib.splitString "\n" text;
        in
        "  ${token}${
            lib.fixedWidthString (lib.max 1 (column - 2 - builtins.stringLength token)) " " ""
          }${builtins.head lines}"
        + lib.concatMapStrings (line: "\n${lib.fixedWidthString (column + 1) " " ""}${line}") (
          builtins.tail lines
        );
    in
    lib.concatStringsSep "\n" (
      map (argument: row (label argument) argument.help.text) (visible facts.arguments)
      ++ [ (row (lib.concatStringsSep ", " facts.helpTokens) "Show this help") ]
    );
}
