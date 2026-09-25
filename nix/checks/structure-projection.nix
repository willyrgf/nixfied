# Compile synthetic policy cases through the same renderer as production.
{ pkgs }:
let
  inherit (pkgs) lib;
  d = import ../meta/declarations.nix;
  field =
    name: value: presence:
    d.field name value { kind = presence; } "Synthetic policy test field.";
  record = name: decoder: fields: {
    identity = d.local name;
    description = "Synthetic Rust projection test.";
    inherit decoder fields;
    producer = "None";
    rust = {
      inherit name;
      file = "fixture.rs";
    };
  };
  text = {
    kind = "Text";
  };
  checked = import ../meta/structure.nix { inherit lib; } {
    inventory = { };
    vocabularies = [ ];
    records = [
      (record "EscapingFixture" "RejectUnknown" [
        (
          (field (builtins.fromJSON ''"wire\u0001\b\f\r\n\t\"\\b\\u0001 λ"'') text "Required")
          // {
            rust.name = "ordinary";
          }
        )
      ])
      (record "PresenceFixture" "RejectUnknown" [
        (field "optional" text "Optional")
        (field "details" {
          kind = "OpenJson";
          description = "Open test details.";
        } "Required")
      ])

    ];
  };
  generated = import ../meta/generated.nix {
    inherit pkgs;
    files = import ../meta/rust.nix { inherit lib; } checked;
  };
in
import ./cargo-fixture.nix { inherit pkgs; } {
  name = "nixfied-structure-projection-check";
  script = ''
    install -m 644 ${generated}/fixture.rs crates/nixfied-manifest/tests/structure_projection.rs
    cat ${./structure-projection.rs} >> crates/nixfied-manifest/tests/structure_projection.rs
    cargo test --offline --locked -p nixfied-manifest --test structure_projection
  '';
}
