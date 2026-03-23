{ pkgs }:
let
  source = builtins.readFile ../../nixfied/framework/runtime/services/postgres/backup.nix;
in
assert !(pkgs.lib.hasInfix "\${pkgs.jq}/bin/jq" source);
assert pkgs.lib.hasInfix "write_backup_manifest_fields()" source;
assert pkgs.lib.hasInfix ".manifest.fields" source;
pkgs.runCommand "postgres-backup-contract" { } ''
  echo "OK: postgres backup runtime renders manifests without jq and writes field sidecars" > "$out"
''
