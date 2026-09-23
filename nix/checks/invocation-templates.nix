{ lib, pkgs, system }:
let
  tokenize = import ../lib/invocation-template.nix { inherit lib; };
  vectors = builtins.fromJSON (builtins.readFile ../../runtime/crates/nixfied-runtime/tests/fixtures/invocation-templates.json);
  accepts = run:
    let
      evaluated = import ../compiler/resolve.nix {
        inherit lib pkgs system;
        module.nixfied.tasks.one.invocation = { tools = [ pkgs.coreutils ]; inherit run; };
      };
      validated = import ../compiler/validate.nix { inherit lib system; } evaluated.config;
    in (builtins.tryEval (builtins.deepSeq validated.nixfied.tasks.one.invocation.run true)).success;
in
assert accepts [ "\${port:absent}" ];
assert accepts [ "\${port:" ];
assert !(accepts [ "tool" "\${port:absent}" ]);
assert !(accepts [ "tool" "\${port:" ]);
assert !(accepts [ "\${secret:key}" ]);
assert builtins.all (vector: tokenize vector.text == vector.tokens) vectors;
true
