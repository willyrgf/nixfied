#!/usr/bin/env bash
# Run from the repository root. Generation and formatting use locked inputs.
set -euo pipefail
# Host Nix only bootstraps the locked evaluator; it does not render the files.
pinned_nix=$(nix build --impure --no-link --print-out-paths --expr '
  let flake = builtins.getFlake ("git+file://" + builtins.getEnv "PWD");
  in flake.inputs.nixpkgs.legacyPackages.${builtins.currentSystem}.nix.out
')
generated=$("$pinned_nix/bin/nix" build --impure --no-link --print-out-paths --expr '
  let
    root = builtins.getEnv "PWD";
    flake = builtins.getFlake ("git+file://" + root);
    pkgs = import flake.inputs.nixpkgs {
      system = builtins.currentSystem;
      overlays = [ flake.inputs.rust-overlay.overlays.default ];
    };
  in import (root + "/nix/meta/generated.nix") { inherit pkgs; }
')
while IFS= read -r -d '' file; do
  relative=${file#"$generated/"}
  mkdir -p "runtime/$(dirname "$relative")"
  install -m 644 "$file" "runtime/$relative"
done < <(find "$generated" -type f -print0)
