# Framework-only app; run from the Nixfied repository root.
{ pkgs }:
let
  generated = import ./generated.nix { inherit pkgs; };
in
pkgs.writeShellApplication {
  name = "nixfied-regenerate";
  runtimeInputs = [
    pkgs.coreutils
    pkgs.findutils
  ];
  inheritPath = false;
  text = ''
    if [[ $# != 0 || ! -f runtime/Cargo.toml || ! -d nix/meta ]]; then
      echo 'Run nix run .#regenerate from the Nixfied repository root, without arguments.' >&2
      exit 2
    fi
    generated=${generated}
    while IFS= read -r -d "" file; do
      relative=''${file#"$generated/"}
      mkdir -p "runtime/$(dirname "$relative")"
      install -m 644 "$file" "runtime/$relative"
    done < <(find "$generated" -type f -print0)
  '';
}
