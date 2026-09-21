{
  pkgs,
  files ? import ./generated-files.nix {
    inherit (pkgs) lib;
    checked = import ./default.nix { inherit (pkgs) lib; };
  },
}:
let
  toolchain = (import ../toolchain.nix { inherit pkgs; }).dev;
in
pkgs.runCommand "nixfied-generated-rust" { nativeBuildInputs = [ toolchain ]; } (
  pkgs.lib.concatStringsSep "\n" (
    pkgs.lib.mapAttrsToList (file: text: ''
      mkdir -p "$out/$(dirname ${pkgs.lib.escapeShellArg file})"
      cp ${builtins.toFile "generated.rs" text} "$out/"${pkgs.lib.escapeShellArg file}
      chmod u+w "$out/"${pkgs.lib.escapeShellArg file}
      rustfmt --edition 2024 "$out/"${pkgs.lib.escapeShellArg file}
    '') files
  )
)
