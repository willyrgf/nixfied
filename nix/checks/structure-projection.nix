# Compile synthetic policy cases through the same renderer as production.
{ pkgs }:
let
  inherit (pkgs) lib;
  d = import ../meta/declarations.nix;
  field =
    name: value: presence:
    d.field name value { kind = presence; } "Synthetic policy test field.";
  record = name: emission: decoder: fields: {
    identity = d.local name;
    description = "Synthetic Rust projection test.";
    inherit decoder fields;
    producer = "None";
    rust = {
      inherit name emission;
      file = "fixture.rs";
      visibility = "private";
      derives = [ "Debug" ];
    };
  };
  text = {
    kind = "Text";
  };
  checked = import ../meta/structure.nix { inherit lib; } {
    inventory = { };
    vocabularies = [ ];
    records = [
      (record "EscapingFixture" "Owned" "RejectUnknown" [
        (
          (field (builtins.fromJSON ''"wire\u0001\b\f\r\n\t\"\\b\\u0001 λ"'') text "Required")
          // {
            rust = {
              name = "ordinary";
              visibility = "private";
              storage = "Direct";
            };
          }
        )
      ])
      (record "PresenceFixture" "Owned" "RejectUnknown" [
        (field "optional" text "Optional")
        (field "details" {
          kind = "OpenJson";
          description = "Open test details.";
        } "Required")
      ])
      (record "BorrowedFixture" "Borrowed" "NoDecoder" [
        (field "values" {
          kind = "List";
          element = text;
          unique = false;
        } "OmitEmpty")
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
