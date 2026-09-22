# Exercise the shipped dispatcher and the same builder with poisoned products.
{
  lib,
  pkgs,
  system,
  docs,
}:
let
  authoring = import ../meta/authoring.nix {
    inherit lib;
    pkgs = throw "static docs forced native package providers";
    system = throw "static docs forced a contextual default";
  };
  sentinel = pkgs.runCommand "nixfied-reference-unrealised-sentinel" { } "exit 1";
  fixture =
    marker:
    import ../docs/reference.nix {
      inherit lib pkgs system;
      options = authoring.options;
      publications =
        (import ../meta/publications.nix { inherit lib; } {
          declarations = [
            {
              kind = "package";
              scope = "root";
              name = "unused";
              description = "${marker}: display-only ${sentinel}";
              usage = "nix build .#unused";
              artifact = "Native fixture artifact.";
              binding = throw "reference forced an unused package";
            }
          ];
        }).entries;
      source = {
        path = "${sentinel}/${marker}";
        revision = null;
      };
    };
  first = fixture "source-one";
  second = fixture "source-two";
  closure = pkgs.closureInfo {
    rootPaths = [
      docs
      first
      second
    ];
  };
in
assert builtins.getContext first.serialized == { };
assert builtins.hasContext "${sentinel}/bin/program";
pkgs.runCommand "nixfied-reference-check" { nativeBuildInputs = [ pkgs.jq ]; } ''
  docs=${docs}/bin/nixfied-docs
  "$docs" > index.txt
  grep -Fq 'Supplying framework source:' index.txt
  grep -Fq 'docs option' index.txt
  "$docs" option 'nixfied.services.<name>.stateRefs' > state.txt
  grep -Fq 'list of string' state.txt
  grep -Fq 'slot' state.txt
  grep -Fq 'Execution lowering discards' state.txt
  grep -Fq 'docs topic state' state.txt
  "$docs" option nixfied.target.system > system.txt
  grep -Fxq 'system' system.txt
  "$docs" options nixfied.services > options.txt
  test "$(wc -l < options.txt)" -eq 75
  "$docs" options 'nixfied.services.<name>.stateRefs' > exact.txt
  test "$(cat exact.txt)" = 'nixfied.services.<name>.stateRefs'
  "$docs" api function > functions.txt
  printf '%s\n' library/compileModel library/projectApps library/seq > expected.txt
  diff -u expected.txt functions.txt
  "$docs" api app project/docs | grep -Fq 'no model admission'
  "$docs" api app root/regenerate | grep -Fq 'nix run .#regenerate'
  "$docs" api package check/rust-workspace | grep -Fq 'Clippy'
  "$docs" api error > errors.txt
  test "$(wc -l < errors.txt)" -eq 27
  "$docs" api error OUTPUT_PROJECTION_FAILED | grep -Fq 'docs topic outputs'
  "$docs" api record output-schema/runtime-error | grep -Fq 'open JSON'
  "$docs" api record local/RegistryIdentityDiagnostic | grep -Fq 'signed'
  "$docs" api record output-schema/run-task | grep -Fq 'IgnoreUnknown'
  "$docs" api command run > run-command.txt
  grep -Fq -- '--allow-non-store-model' run-command.txt
  grep -Fq 'Initial value: 5000' run-command.txt
  grep -Fq 'Help visibility: Hidden' run-command.txt
  "$docs" api command upgrade | grep -Fq 'inverse update_lock'
  "$docs" api app project/run | grep -Fq 'See command run'
  "$docs" topic runtime | grep -Fq 'Open leases are replacement authority'
  "$docs" topic context | grep -Fq 'XDG_CONFIG_HOME/nixfied/secrets'
  "$docs" topic commands | grep -Fq 'before checking duplication'
  "$docs" topic errors | grep -Fq 'Non-object details become an empty object'
  "$docs" topic state > state-topic.txt
  grep -Fq 'arbitrary strings' state-topic.txt
  "$docs" topic placeholders > placeholders.txt
  grep -Fq 'first' placeholders.txt
  "$docs" source | grep -Fq '/nix/store/'
  "$docs" --help > help.txt
  "$docs" -h > short-help.txt
  diff -u help.txt short-help.txt
  PATH=/no-host-tools "$docs" --help > isolated-help.txt
  diff -u help.txt isolated-help.txt
  PATH=/no-host-tools "$docs" options nixfied.target > isolated-options.txt
  grep -Fxq nixfied.target.system isolated-options.txt
  reject() {
    if "$docs" "$@" >out.txt 2>err.txt; then
      echo 'docs accepted an invalid query' >&2; exit 1
    fi
    test ! -s out.txt
    grep -Fq 'use docs --help' err.txt
  }
  reject ""
  reject option
  reject option nixfied.services.stateRefs
  reject option 'nixfied.services.<name>.stateRefs' extra
  reject options nixfied.serv
  reject options nixfied.servicesx
  reject options nixfied.services extra
  reject topic missing
  reject topic state extra
  reject api missing
  reject api function root/compileModel
  reject api app project/regenerate
  reject api function library/compileModel extra
  reject source extra
  reject --help extra
  reject unknown
  # Runtime environment and cwd cannot redirect the realised content.
  mkdir unrelated
  (cd unrelated; NIXFIED_STATE_DIR="$TMPDIR/must-not-exist" "$docs" source > ../elsewhere.txt)
  "$docs" source > here.txt
  diff -u here.txt elsewhere.txt
  test ! -e "$TMPDIR/must-not-exist"
  test -s ${docs}/share/nixfied/reference/API.md
  grep -Fq 'status RunLeaseStatus' ${docs}/share/nixfied/reference/API.md
  ${first}/bin/nixfied-docs source > first.txt
  ${second}/bin/nixfied-docs source > second.txt
  grep -Fq 'source-one' first.txt
  grep -Fq 'source-two' second.txt
  if cmp -s first.txt second.txt; then exit 1; fi
  ${first}/bin/nixfied-docs api package root/unused | grep -Fq 'source-one'
  ${second}/bin/nixfied-docs api package root/unused | grep -Fq 'source-two'
  if grep -E 'nixfied-(runtime|cli)-|nixfied-reference-unrealised-sentinel' ${closure}/store-paths; then
    echo 'reference retained a described executable dependency' >&2; exit 1
  fi
  touch "$out"
''
