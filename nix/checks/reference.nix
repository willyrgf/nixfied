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
    marker: contextTopic:
    import ../docs/reference.nix {
      inherit lib pkgs system;
      options = authoring.options;
      topics = builtins.mapAttrs (_: topic: topic // { select = [ ]; }) (import ../docs/topics.nix);
      publications =
        (import ../meta/publications.nix { inherit lib; } {
          targets = map (id: {
            kind = "topic";
            inherit id;
          }) (builtins.attrNames (import ../docs/topics.nix));
          declarations = [
            {
              kind = "package";
              scope = "root";
              name = "unused";
              description = "${marker}: display-only ${sentinel}";
              usage = "nix build .#unused";
              artifact = "Native fixture artifact.";
              references = [
                {
                  kind = "topic";
                  id = contextTopic;
                }
              ];
              binding = throw "reference forced an unused package";
            }
          ];
        }).entries;
      source = {
        path = "${sentinel}/${marker}";
        revision = null;
      };
    };
  first = fixture "source-one" "runtime";
  second = fixture "source-two" "secrets";
  fragmentOne = builtins.toFile "reference-fragment-one.md" "## Shared\nfirst source\n## Other\nexcluded";
  fragmentTwo = builtins.toFile "reference-fragment-two.md" "## Shared\nsecond source";
  fragmentSelections = [
    {
      file = fragmentTwo;
      heading = "## Shared";
    }
    {
      file = fragmentOne;
      heading = "## Shared";
    }
  ];
  composed = docs.composeFragments fragmentSelections;
  rejectedFragments =
    fragments: !(builtins.tryEval (builtins.deepSeq (docs.composeFragments fragments) true)).success;
  # Exercise provenance collisions through the complete serialization boundary.
  # Either source alone is valid, so the combined rejection isolates basename identity.
  rootReadme = {
    file = ../../README.md;
    heading = "# Nixfied";
  };
  downstreamReadme = {
    file = ../../examples/downstream/README.md;
    heading = "# Downstream Worked Example";
  };
  documentFixture =
    fragments:
    (import ../docs/reference.nix {
      inherit lib pkgs system;
      options = [ ];
      publications = [ ];
      syntax.commands = [ ];
      structure = {
        records = [ ];
        vocabularies = [ ];
        vocabularyMap.error-code.annotations = { };
      };
      topics.provenance = {
        inherit fragments;
        select = [ ];
        related = [ ];
      };
      source = {
        path = "document-provenance-fixture";
        revision = null;
      };
    }).serialized;
  fenceVectors = [
    {
      document = "## Selected\n    ```\n## Next\nexcluded";
      expected = "## Selected\n    ```";
    }
    {
      document = "## Selected\n    ~~~\n## Next\nexcluded";
      expected = "## Selected\n    ~~~";
    }
    {
      document = "## Selected\n```\n## Hidden\n```\n## Next\nexcluded";
      expected = "## Selected\n```\n## Hidden\n```";
    }
    {
      document = "## Selected\n ~~~\n## Hidden\n ~~~\n## Next\nexcluded";
      expected = "## Selected\n ~~~\n## Hidden\n ~~~";
    }
    {
      document = "## Selected\n  ```\n## Hidden\n  ```\n## Next\nexcluded";
      expected = "## Selected\n  ```\n## Hidden\n  ```";
    }
    {
      document = "## Selected\n   ~~~\n## Hidden\n   ~~~\n## Next\nexcluded";
      expected = "## Selected\n   ~~~\n## Hidden\n   ~~~";
    }
    {
      document = "## Selected\n```text\n## Hidden\n```\t\n## Next\nexcluded";
      expected = "## Selected\n```text\n## Hidden\n```\t";
    }
    {
      document = "## Selected\n~~~text\n## Hidden\n~~~ \t\n## Next\nexcluded";
      expected = "## Selected\n~~~text\n## Hidden\n~~~ \t";
    }
    {
      document = "## Selected\n```\n## Hidden\n````\n## Next\nexcluded";
      expected = "## Selected\n```\n## Hidden\n````";
    }
    {
      document = "## Selected\n~~~~\n~~~\n## Hidden\n~~~~\n## Next\nexcluded";
      expected = "## Selected\n~~~~\n~~~\n## Hidden\n~~~~";
    }
    {
      document = "## Selected\n```\n~~~\n## Hidden\n```\n## Next\nexcluded";
      expected = "## Selected\n```\n~~~\n## Hidden\n```";
    }
    {
      document = "## Selected\n```bad`info\n## Next\nexcluded";
      expected = "## Selected\n```bad`info";
    }
    {
      document = "## Selected\n~~~info`allowed\n## Hidden\n~~~\n## Next\nexcluded";
      expected = "## Selected\n~~~info`allowed\n## Hidden\n~~~";
    }
    {
      document = "## Selected\n```\n    ```\n## Hidden\n```\n## Next\nexcluded";
      expected = "## Selected\n```\n    ```\n## Hidden\n```";
    }
  ];
  closure = pkgs.closureInfo {
    rootPaths = [
      docs
      first
      second
    ];
  };
in
assert import ./docs-navigation.nix { inherit lib; };
assert import ./reference-content.nix { inherit lib; index = builtins.fromJSON docs.serialized; };
# These literal results are the independent fence-boundary expectations.
assert lib.all (
  vector: docs.extractSection vector.document "## Selected" == vector.expected
) fenceVectors;
assert
  (builtins.fromJSON (documentFixture [ rootReadme ])).documents."README.md"
  == builtins.readFile rootReadme.file;
assert
  (builtins.fromJSON (documentFixture [ downstreamReadme ])).documents."README.md"
  == builtins.readFile downstreamReadme.file;
assert
  !(builtins.tryEval (
    builtins.deepSeq (documentFixture [
      rootReadme
      downstreamReadme
    ]) true
  )).success;
assert
  docs.extractSection "# Title\n## Selected\nbody\n### Child\nchild\n```sh\n## Fake\n```\n~~~\n## Fake too\n~~~\n## Next\nexcluded" "## Selected"
  == "## Selected\nbody\n### Child\nchild\n```sh\n## Fake\n```\n~~~\n## Fake too\n~~~";
assert docs.extractSection "## Last\nbody" "## Last" == "## Last\nbody";
assert !(builtins.tryEval (docs.extractSection "## Other" "## Missing")).success;
assert !(builtins.tryEval (docs.extractSection "## Same\n## Same" "## Same")).success;
assert composed.prose == "## Shared\nsecond source\n## Shared\nfirst source";
assert
  composed.fragments == map (fragment: {
    document = builtins.baseNameOf fragment.file;
    inherit (fragment) heading;
  }) fragmentSelections;
assert rejectedFragments [ ];
assert rejectedFragments null;
assert rejectedFragments [ "not a fragment" ];
assert rejectedFragments [
  {
    file = 42;
    heading = "## Shared";
  }
];
assert rejectedFragments [ { file = fragmentOne; } ];
assert rejectedFragments [
  {
    file = fragmentOne;
    heading = "## Shared";
    extra = true;
  }
];
assert rejectedFragments [
  {
    file = fragmentOne;
    heading = "";
  }
];
assert rejectedFragments [
  (builtins.head fragmentSelections)
  (builtins.head fragmentSelections)
];
assert rejectedFragments [
  {
    file = fragmentOne;
    heading = "## Missing";
  }
];
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
    grep -Fxq 'nixfied.services.<name>.endpoint.readyProbe.run' options.txt
    grep -Fxq 'nixfied.services.<name>.endpoints.<name>.healthProbe.run' options.txt
    grep -Fxq 'nixfied.services.<name>.lifecycle.ready.policy.maxAttempts' options.txt
    if grep -Fq '.probe.kind' options.txt; then exit 1; fi
    "$docs" options 'nixfied.services.<name>.stateRefs' > exact.txt
    test "$(cat exact.txt)" = 'nixfied.services.<name>.stateRefs'
    "$docs" api function > functions.txt
    printf '%s\n' library/compileManifest library/projectApps library/seq > expected.txt
    diff -u expected.txt functions.txt
    "$docs" api app project/docs | grep -F 'no manifest admission' > /dev/null
    "$docs" api app root/regenerate | grep -F 'nix run .#regenerate' > /dev/null
    "$docs" api package check/rust-workspace | grep -F 'Clippy' > /dev/null
    "$docs" api error > errors.txt
    test "$(wc -l < errors.txt)" -eq 25
    "$docs" api error OUTPUT_PROJECTION_FAILED | grep -F 'docs topic outputs' > /dev/null
    "$docs" api record output-schema/runtime-error | grep -F 'open JSON' > /dev/null
    "$docs" api record local/RegistryIdentityDiagnostic | grep -F 'signed' > /dev/null
    "$docs" api record output-schema/run-task | grep -F 'IgnoreUnknown' > /dev/null
    "$docs" api command run > run-command.txt
    grep -Fq -- '--allow-non-store-manifest' run-command.txt
    grep -Fq 'Initial value: 5000' run-command.txt
    grep -Fq 'Help visibility: Hidden' run-command.txt
    "$docs" api command upgrade | grep -F 'inverse update_lock' > /dev/null
    "$docs" api app project/run | grep -F 'See command run' > /dev/null
    "$docs" topic runtime > runtime-topic.txt
    grep -Fq 'Slot mutation has one owner' runtime-topic.txt
    grep -Fq 'Admission correctness (Rust)' runtime-topic.txt
    grep -Fq 'Execution correctness (Rust)' runtime-topic.txt
    grep -Fq 'docs topic state' runtime-topic.txt
    if grep -Eq '^## (The problem|Shared contracts|Verification boundary)' runtime-topic.txt; then
      echo 'runtime topic leaked unrelated sections' >&2; exit 1
    fi
    # Static expectations are checked independently in Nix; exercise every
    # shipped topic through the actual dispatcher as well.
    index=${docs}/share/nixfied/reference/index.json
    while IFS= read -r topic; do
      "$docs" topic "$topic" > actual-topic.txt
      jq -r --arg topic "$topic" '.topics[$topic].text' "$index" > expected-topic.txt
      diff -u expected-topic.txt actual-topic.txt
    done < <(jq -r '.topics | keys[]' "$index")
    "$docs" topic context | grep -F 'XDG_CONFIG_HOME/nixfied/secrets' > /dev/null
    "$docs" topic commands | grep -F 'before checking duplication' > /dev/null
    "$docs" topic errors | grep -F 'Non-object details become an empty object' > /dev/null
    "$docs" topic state > state-topic.txt
    grep -Fq 'These labels are not a storage-backend selector' state-topic.txt
    if grep -Fq '## Source and invocation context' state-topic.txt || grep -Fxq '## Secrets' state-topic.txt; then
      echo 'state topic leaked context or secret sections' >&2; exit 1
    fi
    "$docs" topic placeholders > placeholders.txt
    grep -Fq 'first' placeholders.txt
    "$docs" source | grep -F '/nix/store/' > /dev/null
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
    reject api function root/compileManifest
    reject api app project/regenerate
    reject api function library/compileManifest extra
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
    grep -Fxq '## Changing the contract' ${docs}/share/nixfied/reference/API.md
    grep -Fxq '#### V10 — servicesRequired: connectsTo closure is a fixpoint' ${docs}/share/nixfied/reference/API.md
    grep -Fxq '## Verification boundary' ${docs}/share/nixfied/reference/API.md
    ${first}/bin/nixfied-docs source > first.txt
    ${second}/bin/nixfied-docs source > second.txt
    grep -Fq 'source-one' first.txt
    grep -Fq 'source-two' second.txt
    if cmp -s first.txt second.txt; then exit 1; fi
    ${first}/bin/nixfied-docs api package root/unused | grep -F 'source-one' > /dev/null
    ${second}/bin/nixfied-docs api package root/unused | grep -F 'source-two' > /dev/null
    ${first}/bin/nixfied-docs topic runtime > first-topic.txt
    ${second}/bin/nixfied-docs topic secrets > second-topic.txt
    grep -Fq 'source-one: display-only' first-topic.txt
    grep -Fq 'source-two: display-only' second-topic.txt
    ${first}/bin/nixfied-docs api package root/unused | grep -F 'docs topic runtime' > /dev/null
    ${second}/bin/nixfied-docs api package root/unused | grep -F 'docs topic secrets' > /dev/null
    ${second}/bin/nixfied-docs topic runtime > moved-topic.txt
    if grep -Fq '### package root/unused' moved-topic.txt; then
      echo 'changed declaration left stale topic membership' >&2; exit 1
    fi
    if grep -E 'nixfied-(runtime|cli)-|nixfied-reference-unrealised-sentinel' ${closure}/store-paths; then
      echo 'reference retained a described executable dependency' >&2; exit 1
    fi
    touch "$out"
''
