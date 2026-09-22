# Run from the repository root through the pinned development shell.
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
      echo 'Run nix develop --command nixfied-regenerate from the repository root, without arguments.' >&2
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
