# Product-specific freshness: builds themselves compile the checked-in sources.
{
  pkgs,
  package,
  source ? ../../runtime,
}:
let
  checked = import ../meta/default.nix { inherit (pkgs) lib; };
  files = import ../meta/generated-files.nix {
    inherit (pkgs) lib;
    inherit checked;
  };
  generated = import ../meta/generated.nix { inherit pkgs files; };
  members =
    (import ../packages/runtime-source.nix {
      inherit pkgs package source;
      lockFile = ../../runtime/Cargo.lock;
    }).selectedMembers;
  selected = builtins.filter (
    file: pkgs.lib.any (member: pkgs.lib.hasPrefix "${member}/" file) members
  ) (builtins.attrNames files);
in
pkgs.runCommand "${package}-generated-freshness" { } (
  pkgs.lib.concatMapStringsSep "\n" (file: ''
    diff -u ${source + "/${file}"} ${generated}/${file}
  '') selected
  + ''
    touch "$out"
  ''
)
