{ pkgs }:
let
  upgrade = import ../install/upgrade.nix { inherit pkgs; };
in
pkgs.runCommand "nixfied-upgrade-syntax" { nativeBuildInputs = [ pkgs.python3 ]; } ''
  python3 ${./upgrade-syntax.py} ${upgrade}/bin/nixfied-upgrade ${../fixtures/upgrade-help.txt}
  touch "$out"
''
