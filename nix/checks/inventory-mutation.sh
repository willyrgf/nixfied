# Mutate the actual single authored inventory in an isolated current-source copy.
local work source capability
work=$(mktemp -d)
source=$(nix flake metadata --no-write-lock-file --json "$checkout" | jq -er .path)
cp -R "$source/." "$work/"
chmod -R u+w "$work"
capability=$(<"$work/runtime/crates/nixfied-model/capability.txt")
printf '%s\n' "${capability/enum StdinPolicy: null inherit/enum StdinPolicy: null pipe}" \
  > "$work/runtime/crates/nixfied-model/capability.txt"
NIXFIED_MUTATION_SOURCE="$work" nix build --impure --no-link --expr '
  let
    root = builtins.getEnv "NIXFIED_MUTATION_SOURCE";
    f = builtins.getFlake ("path:" + root);
    pkgs = import f.inputs.nixpkgs {
      system = builtins.currentSystem;
      overlays = [ f.inputs.rust-overlay.overlays.default ];
    };
  in import (root + "/nix/checks/inventory-mutation.nix") { inherit pkgs; }
' || fail 'inventory mutation: Nix/Rust projection failed'
nix run --no-write-lock-file "path:$work#docs" -- option 'nixfied.tasks.<name>.invocation.stdin' > "$work/option.txt"
grep -Fq '"pipe"' "$work/option.txt" || fail 'inventory mutation: native option reference did not change'
if grep -Fq '"inherit"' "$work/option.txt"; then fail 'inventory mutation: stale native enum members'; fi
nix run --no-write-lock-file "path:$work#docs" -- api record primitive/Invocation > "$work/record.txt"
grep -Fq 'enum StdinPolicy: null, pipe' "$work/record.txt" || fail 'inventory mutation: record reference did not change'
if grep -Fq 'enum StdinPolicy: null, inherit' "$work/record.txt"; then fail 'inventory mutation: stale record members'; fi
rm -rf "$work"
