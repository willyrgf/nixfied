# Executable source-identity matrix. Only current checkout files are copied.
local work source variant product
work=$(mktemp -d)
source=$(nix flake metadata --no-write-lock-file --json "$checkout" | jq -er .path)
cat > "$work/identity.nix" <<'NIX'
{ framework, variant }:
let
  flake = builtins.getFlake ("path:" + framework);
  pkgs = import flake.inputs.nixpkgs {
    system = builtins.currentSystem;
    overlays = [ flake.inputs.rust-overlay.overlays.default ];
  };
  product = package:
    let built = import (variant + "/nix/packages/runtime.nix") {
      inherit pkgs package; source = builtins.toPath (variant + "/runtime");
    };
    in { source = built.src.drvPath; derivation = built.drvPath; };
in builtins.listToAttrs (map (name: { inherit name; value = product name; })
  [ "nixfied-cli" "nixfied-runtime" "nixfied-test-child" ])
NIX
for variant in baseline cli runtime model generated runtime-generated cli-generated child reference declaration; do
  mkdir -p "$work/$variant"
  cp -R "$source/." "$work/$variant/"
  chmod -R u+w "$work/$variant"
  case "$variant" in
    cli) printf '\n// CLI source variation.\n' >> "$work/$variant/runtime/crates/nixfied-cli/src/main.rs" ;;
    runtime) printf '\n// Runtime source variation.\n' >> "$work/$variant/runtime/crates/nixfied-runtime/src/main.rs" ;;
    model) printf '\n// Native model source variation.\n' >> "$work/$variant/runtime/crates/nixfied-model/src/types.rs" ;;
    generated) printf '\n// Generated model source variation.\n' >> "$work/$variant/runtime/crates/nixfied-model/src/generated/types.rs" ;;
    runtime-generated) printf '\n// Generated output source variation.\n' >> "$work/$variant/runtime/crates/nixfied-runtime/src/generated/output.rs" ;;
    cli-generated) printf '\n// Generated CLI source variation.\n' >> "$work/$variant/runtime/crates/nixfied-cli/src/generated/commands.rs" ;;
    child) printf '\n// Test child source variation.\n' >> "$work/$variant/runtime/crates/nixfied-test-child/src/main.rs" ;;
    reference) printf '\nReference variation\n' >> "$work/$variant/docs/GUIDE.md" ;;
    declaration) printf '\n# Declaration variation\n' >> "$work/$variant/nix/meta/model.nix" ;;
  esac
  NIXFIED_MATRIX_EXPR="$work/identity.nix" NIXFIED_MATRIX_FRAMEWORK="$source" NIXFIED_MATRIX_VARIANT="$work/$variant" \
    nix eval --impure --json --expr 'import (builtins.getEnv "NIXFIED_MATRIX_EXPR") {
      framework = builtins.getEnv "NIXFIED_MATRIX_FRAMEWORK";
      variant = builtins.getEnv "NIXFIED_MATRIX_VARIANT";
    }'  > "$work/$variant.json" || fail 'source matrix evaluation failed'
done
for variant in cli runtime model generated runtime-generated cli-generated child reference declaration; do
  for product in nixfied-cli nixfied-runtime nixfied-test-child; do
    local changed=false
    case "$variant:$product" in
      cli:nixfied-cli|cli-generated:nixfied-cli|runtime:nixfied-runtime|model:nixfied-runtime|generated:nixfied-runtime|runtime-generated:nixfied-runtime|child:nixfied-test-child) changed=true ;;
    esac
    jq -en --arg product "$product" --argjson changed "$changed" \
      --slurpfile baseline "$work/baseline.json" --slurpfile variant "$work/$variant.json" '
        ($baseline[0][$product].source != $variant[0][$product].source) == $changed and
        ($baseline[0][$product].derivation != $variant[0][$product].derivation) == $changed
      ' >/dev/null || fail "source isolation: $variant affected the wrong product $product"
  done
done
rm -rf "$work"
