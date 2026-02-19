{
  pkgs,
  model,
}:
let
  rendered = builtins.concatStringsSep "\n" model.views.help.lines + "\n";
  expected = builtins.readFile ./snapshots/help.txt;
in
assert rendered == expected;
pkgs.runCommand "v2-help-snapshot" { } ''
  echo "OK: help snapshot is stable" > "$out"
''
