# Compile synthetic policy cases through the same renderer as production.
{ pkgs }:
let
  inherit (pkgs) lib;
  field = name: value: decode: rustEncode: {
    inherit
      name
      value
      decode
      rustEncode
      ;
    description = "Synthetic policy test field.";
    nixEncode = "NotProduced";
    rust = {
      visibility = "private";
      storage = "Direct";
    };
  };
  record = name: emission: decoder: fields: {
    identity = {
      kind = "Local";
      inherit name;
    };
    description = "Synthetic Rust projection test.";
    inherit decoder fields;
    rust = {
      inherit name emission;
      file = "fixture.rs";
      module = [ "fixture" ];
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
          (field (builtins.fromJSON ''"wire\u0001\b\f\r\n\t\"\\b\\u0001 λ"'') text {
            kind = "Required";
          } "Present")
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
        (field "nullable" text { kind = "Nullable"; } "Present")
        (field "optional" text { kind = "Optional"; } "Present")
        (field "details" {
          kind = "OpenJson";
          description = "Open test details.";
        } { kind = "Required"; } "Present")
      ])
      (record "BorrowedFixture" "Borrowed" "NoDecoder" [
        (field "values" {
          kind = "List";
          element = text;
          unique = false;
        } { kind = "Required"; } "OmitEmpty")
      ])
    ];
  };
  generated = import ../meta/generated.nix {
    inherit pkgs;
    files = import ../meta/rust.nix { inherit lib; } checked;
  };
  toolchain = (import ../toolchain.nix { inherit pkgs; }).dev;
in
pkgs.stdenv.mkDerivation {
  name = "nixfied-structure-projection-check";
  src = ../../runtime;
  cargoDeps = pkgs.rustPlatform.importCargoLock { lockFile = ../../runtime/Cargo.lock; };
  nativeBuildInputs = [
    toolchain
    pkgs.rustPlatform.cargoSetupHook
  ];
  buildPhase = ''
    install -m 644 ${generated}/fixture.rs crates/nixfied-model/tests/structure_projection.rs
    cat ${./structure-projection.rs} >> crates/nixfied-model/tests/structure_projection.rs
    cargo test --offline --locked -p nixfied-model --test structure_projection
  '';
  installPhase = ''touch "$out"'';
}
