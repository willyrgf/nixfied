# Compile unusual text defaults through the production literal renderer.
{ pkgs }:
let
  inherit (pkgs) lib;
  structure = import ../meta/default.nix { inherit lib; };
  declarations = import ../meta/commands.nix { inherit lib structure; };
  bytes = builtins.fromJSON ''"\u0001\b\f\r\n\t\"\\b\\u0001 λ"'';
  checked = import ../meta/syntax.nix { inherit lib; } {
    inherit structure;
    declarations = map (
      command:
      command
      // {
        arguments = map (
          arg:
          if arg.id == "root" then
            arg
            // {
              initialValue = {
                kind = "Literal";
                value = bytes;
              };
            }
          else
            arg
        ) command.arguments;
      }
    ) (builtins.filter (command: command.name == "install") declarations);
    inventory = import ../meta/inventory.nix { inherit lib; } (
      builtins.readFile ../../runtime/crates/nixfied-model/capability.txt
    );
    topics = builtins.attrNames (import ../docs/topics.nix);
    publications = [ ];
  };
  projection = import ../meta/syntax-project.nix { inherit lib structure; } checked;
  source = pkgs.writeText "syntax-projection.rs" (
    ''
      #![allow(dead_code)] // The fixture inspects the text literal, not every constant.
    ''
    + projection.rust [ "install" ]
    + ''
      #[test]
      fn preserves_text_bytes() {
        assert_eq!(INSTALL_ROOT_INITIAL, "\u{1}\u{8}\u{c}\r\n\t\"\\b\\u0001 λ");
      }
    ''
  );
  toolchain = (import ../toolchain.nix { inherit pkgs; }).dev;
in
pkgs.runCommand "nixfied-syntax-projection" { nativeBuildInputs = [ toolchain ]; } ''
  rustc --edition 2024 --test ${source} -o projection-test
  ./projection-test
  touch "$out"
''
