# Read authored coordinates without assigning structure or omission semantics.
{ lib }:
text:
let
  lines = builtins.filter (
    line: line != "" && !(lib.hasPrefix "#" line) && line != "nixfied capability descriptor"
  ) (lib.splitString "\n" text);
  parse =
    line:
    let
      match = builtins.match "([^:]+):[[:space:]]*(.*)" line;
    in
    assert lib.assertMsg (match != null) "Nixfied inventory: malformed line";
    let
      name = builtins.elemAt match 0;
      members = builtins.filter (word: word != "") (lib.splitString " " (builtins.elemAt match 1));
    in
    assert lib.assertMsg (
      members != [ ] && lib.unique members == members
    ) "Nixfied inventory ${name}: empty or duplicate members";
    {
      inherit name;
      value = members;
    };
  entries = map parse lines;
  names = map (entry: entry.name) entries;
in
assert lib.assertMsg (lib.unique names == names) "Nixfied inventory: duplicate coordinate";
builtins.listToAttrs entries
