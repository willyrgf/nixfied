{ pkgs }:
let
  canonical = import ../../nixfied/framework/core/canonical.nix { inherit (pkgs) lib; };
  contracts = import ../../nixfied/contracts {
    inherit canonical;
    inherit (pkgs) lib;
  };
  fixture = import ./lib/contract-render-fixture.nix { inherit contracts; };

  jsonRendered = contracts.renderJsonSchema.bundle fixture;
  docsRendered = contracts.renderDocs.bundle fixture;

  jsonExpected = builtins.readFile ./snapshots/contracts/example.json;
in
assert jsonRendered == jsonExpected;
assert pkgs.lib.hasInfix "machineOutput.result" docsRendered;
assert pkgs.lib.hasInfix "runtime.summary.payload" docsRendered;
pkgs.runCommand "contract-render-snapshot" { } ''
  echo "OK: contract renderers are deterministic" > "$out"
''
