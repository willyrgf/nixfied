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
    test "$(wc -l < options.txt)" -eq 75
    "$docs" options 'nixfied.services.<name>.stateRefs' > exact.txt
    test "$(cat exact.txt)" = 'nixfied.services.<name>.stateRefs'
    "$docs" api function > functions.txt
    printf '%s\n' library/compileModel library/projectApps library/seq > expected.txt
    diff -u expected.txt functions.txt
    "$docs" api app project/docs | grep -F 'no model admission' > /dev/null
    "$docs" api app root/regenerate | grep -F 'nix run .#regenerate' > /dev/null
    "$docs" api package check/rust-workspace | grep -F 'Clippy' > /dev/null
    "$docs" api error > errors.txt
    test "$(wc -l < errors.txt)" -eq 27
    "$docs" api error OUTPUT_PROJECTION_FAILED | grep -F 'docs topic outputs' > /dev/null
    "$docs" api record output-schema/runtime-error | grep -F 'open JSON' > /dev/null
    "$docs" api record local/RegistryIdentityDiagnostic | grep -F 'signed' > /dev/null
    "$docs" api record output-schema/run-task | grep -F 'IgnoreUnknown' > /dev/null
    "$docs" api command run > run-command.txt
    grep -Fq -- '--allow-non-store-model' run-command.txt
    grep -Fq 'Initial value: 5000' run-command.txt
    grep -Fq 'Help visibility: Hidden' run-command.txt
    "$docs" api command upgrade | grep -F 'inverse update_lock' > /dev/null
    "$docs" api app project/run | grep -F 'See command run' > /dev/null
    "$docs" topic runtime > runtime-topic.txt
    grep -Fq 'Open leases are replacement authority' runtime-topic.txt
    grep -Fq 'Admission correctness (Rust)' runtime-topic.txt
    grep -Fq 'Execution correctness (Rust)' runtime-topic.txt
    grep -Fq 'docs topic state' runtime-topic.txt
    if grep -Eq '^## (The problem|Shared contracts|Verification boundary)' runtime-topic.txt; then
      echo 'runtime topic leaked unrelated sections' >&2; exit 1
    fi
    # Source-composition integration check; literal vectors above prove fence boundaries.
    ${pkgs.python3}/bin/python3 - "$docs" ${docs}/share/nixfied/reference/index.json <<'PYTHON'
  import json, re, subprocess, sys
  with open(sys.argv[2]) as source:
      index = json.load(source)
  for name, topic in index['topics'].items():
      expected = []
      for fragment in topic['fragments']:
          heading = fragment['heading']
          lines = index['documents'][fragment['document']].splitlines()
          start = lines.index(heading)
          level = len(heading.split(' ', 1)[0])
          end = start + 1
          fence = None
          while end < len(lines):
              line = lines[end]
              marker = re.match(r' {0,3}(`{3,}|~{3,})(.*)$', line)
              if marker:
                  token, tail = marker.groups()
                  if fence is None:
                      if token[0] == '~' or '`' not in tail:
                          fence = token
                  elif token.startswith(fence) and re.fullmatch(r'[ \t]*', tail):
                      fence = None
              elif fence is None and re.match(r'#{1,' + str(level) + r'} ', line):
                  break
              end += 1
          expected.append('\n'.join(lines[start:end]).rstrip())
      output = subprocess.check_output([sys.argv[1], 'topic', name], text=True)
      assert topic['prose'].strip() == '\n\n'.join(expected), name
      assert output.startswith('Topic: ' + name + '\n\n' + topic['prose']), name
      for related in topic['related']:
          assert related['kind'] == 'topic'
          assert related['id'] in index['topics']
          assert 'docs topic ' + related['id'] in output
  # Hand-authored topic expectations are independent of the selector renderer.
  cases = {
      'adapters': ('module', 'adapter/postgres'),
      'authoring': ('argument', 'module-argument/pkgs'),
      'commands': ('command', 'install'),
      'context': ('option', 'nixfied.codebases.main.sourceMode'),
      'derivation': ('function', 'library/seq'),
      'development': ('app', 'root/regenerate'),
      'discovery': ('app', 'project/docs'),
      'errors': ('error', 'SECRET_UNAVAILABLE'),
      'model': ('function', 'library/compileModel'),
      'outputs': ('record', 'output-schema/run-summary-json'),
      'placeholders': ('error', 'PORT_CONFLICT'),
      'recovery': ('command', 'upgrade'),
      'runtime': ('command', 'run'),
      'secrets': ('option', 'nixfied.secrets.<name>.source.kind'),
      'services': ('record', 'primitive/Lifecycle'),
      'state': ('option', 'nixfied.state.cleanupPolicy'),
      'tasks': ('option', 'nixfied.tasks.<name>.requires'),
  }
  assert set(cases) == set(index['topics'])
  for name, required in cases.items():
      topic = index['topics'][name]
      members = {(entry['kind'], entry['id']) for entry in topic['members']}
      assert required in members, (name, required)
      # Development tooling must not enter other topics; runtime APIs must not
      # enter development simply because it links to the model topic.
      excluded = ('command', 'run') if name == 'development' else ('app', 'root/regenerate')
      assert excluded not in members, (name, excluded)
      output = subprocess.check_output([sys.argv[1], 'topic', name], text=True)
      required_text = ('docs api error ' + required[1] if required[0] == 'error'
                       else '### ' + required[0] + ' ' + required[1])
      assert required_text in output, name
      assert '### ' + excluded[0] + ' ' + excluded[1] not in output, name
  # Reader journeys need an explanation of ownership and actions, not only
  # structurally valid membership. Normalize wrapping without hiding omissions.
  prose = {name: ' '.join(topic['prose'].split())
           for name, topic in index['topics'].items()}
  journeys = {
      'state': [
          'Run `down` first.',
          'purge relaxes only that policy gate',
          'never the ownership, confinement, or live-process checks',
          'runtime-owned slot root',
          'Child-tool caches remain project-owned.',
      ],
      'runtime': [
          'projectId / environment / slot / runId',
          'Service identity is layered',
          'Rust materialises *host-absolute* placement at admission',
      ],
      'adapters': [
          'An adapter is a Nix module',
          'imports = [ adapters.postgres ];',
          'the adopter references them as steps in its own composites',
          'contributes *definitions only*',
      ],
      'derivation': [
          'Status: **normative**.',
          'neither implementation may drift from the text',
          'flatten(ci) =',
          'ci.check.fmt',
          'servicesRequired(all) = ["api", "postgres", "worker"]',
          'operationBindings(gitC) = []',
      ],
      'tasks': [
          'Invocations observe the live workspace by default',
      ],
      'development': [
          'nix run .#gate -- --dirty',
          'does not consume other uncommitted framework changes',
          'Use the smallest proof that covers the change',
          'Contract or cross-layer change',
      ],
  }
  for name, explanations in journeys.items():
      for explanation in explanations:
          assert explanation in prose[name], (name, explanation)
  derivation = index['topics']['derivation']['prose']
  assert derivation.index('## 6. Golden vectors') < derivation.index('### 6.1 Representative examples')
  assert set(re.findall(r'^#### V(\d+) ', derivation, re.M)) == {str(n) for n in range(1, 11)}
  assert 'Requesting `task-output` for a composite is rejected' in prose['outputs']
  for name, topic in index['topics'].items():
      output = topic['text']
      assert all(related['id'] != name for related in topic['related']), name
      if topic['related'] and topic['members']:
          assert output.index('### Related topics') < output.index('## Related definitions'), name
      errors = [member['id'] for member in topic['members'] if member['kind'] == 'error']
      if errors:
          assert '### Error codes and related topics' in output, name
          assert '### error ' not in output, name
          for error in errors:
              blocks = re.findall(r'^- `([^`]+)`\n(.*?)(?=^- `|^### |\Z)', output, re.M | re.S)
              matches = [body for code, body in blocks if code == error]
              assert len(matches) == 1, (name, error)
              body = matches[0]
              entry = next(entry for entry in index['api'] if entry['kind'] == 'error' and entry['id'] == error)
              assert entry['description'] in body, (name, error)
              assert 'Details: `docs api error ' + error + '`' in body, (name, error)
              destination = ('This topic' if entry['contextTopic'] == name
                             else '`docs topic ' + entry['contextTopic'] + '`')
              assert 'Related topic: ' + destination in body, (name, error)
              assert 'docs topic ' + name + '`' not in body, (name, error)
  state = index['topics']['state']['text']
  assert state.index('### option nixfied.state.cleanupPolicy') < state.index('### option nixfied.placement.ports.base')
  errors = index['topics']['errors']['text']
  assert errors.index('### record output-schema/runtime-error\n') < errors.index('### record output-schema/run-task\n')
  entries = {(x['kind'], x['id']): x for x in index['api'] + index['options']}
  def refs(key, direction):
      return {(x['kind'], x['id']) for x in entries[key][direction]}
  assert ('command', 'run') in refs(('app', 'project/run'), 'references')
  assert ('app', 'project/run') in refs(('command', 'run'), 'backlinks')
  assert ('record', 'output-schema/run-json') in refs(('command', 'run'), 'references')
  assert ('command', 'run') in refs(('record', 'output-schema/run-json'), 'backlinks')
  assert ('record', 'primitive/TaskSpec') in refs(('record', 'primitive/Model'), 'references')
  assert ('record', 'primitive/Model') in refs(('record', 'primitive/TaskSpec'), 'backlinks')
  target = entries[('option', 'nixfied.target.system')]
  assert ('topic', 'context') in refs(('option', target['id']), 'references')
  assert 'docs topic state' not in target['text']
  assert 'docs topic placeholders' not in target['text']
  assert 'docs topic derivation' not in entries[('record', 'output-schema/runtime-error')]['text']
  for option, topic in [('nixfied.services.<name>.stateRefs', 'state'),
                        ('nixfied.tasks.<name>.requires', 'placeholders')]:
      entry = entries[('option', option)]
      assert 'docs topic ' + topic not in entry['description'], option
      assert 'docs topic ' + topic in entry['text'], option
      assert 'docs topic ' + topic + '`' not in index['topics'][topic]['text'], topic
  outputs = index['topics']['outputs']
  assert {fragment['document'] for fragment in outputs['fragments']} == {'GUIDE.md', 'CONTRACT.md'}
  assert outputs['fragments'][0]['document'] == 'GUIDE.md'
  assert outputs['fragments'][-1]['document'] == 'CONTRACT.md'
  assert 'For an interactive run, use `summary`' in outputs['prose']
  assert outputs['prose'].index('### Choose output') < outputs['prose'].index('## Output and failure contract')
  assert '--output json' in outputs['prose']
  assert '--output task-output' in outputs['prose']
  assert '### 6.1' in index['topics']['derivation']['prose']
  error_record = entries[('record', 'output-schema/runtime-error')]
  error_backlinks = [ref['id'] for ref in error_record['backlinks'] if ref['kind'] == 'error']
  rendered_backlinks = error_record['text'].split('### Referenced by', 1)[1]
  assert set(re.findall(r'^- `([^`]+)`$', rendered_backlinks, re.M)) == set(error_backlinks)
  assert rendered_backlinks.count('docs api error <code>') == 1

  PYTHON
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
