# One static presentation builder for root and project docs. Native values never
# cross this boundary: only normalised documentation and source identity do.
{
  lib,
  pkgs,
  system,
  options,
  publications,
  source,
  topics ? import ./topics.nix,
  structure ? import ../meta/default.nix { inherit lib; },
  syntax ? import ../meta/command-default.nix { inherit lib; },
}:
let
  commandText = command: ''
    ## command ${command.name}

    ${command.description}

    ```text
    ${syntax.help.${command.name}}
    ```

    ${lib.concatMapStringsSep "\n\n" (argument: ''
      ### ${argument.token}

      ${if argument.help.kind == "Visible" then argument.help.text else argument.help.explanation}

      Domain: ${argument.valueDomain.kind}${
        lib.optionalString (argument.valueDomain.kind == "Unsigned") (toString argument.valueDomain.bits)
      }${
        lib.optionalString (argument.valueDomain.kind == "Enum") " (${argument.valueDomain.coordinate})"
      }.
      Initial value: ${
        if argument.initialValue.kind == "Absent" then
          "Absent"
        else
          builtins.toJSON argument.initialValue.value
      }.
      Help visibility: ${argument.help.kind}.
    '') command.arguments}

    Acquisition, repetition, encoding, help precedence and effects: `docs topic commands`.
  '';
  valueText =
    value:
    if value.kind == "Enum" then
      "${value.coordinate}: ${
        lib.concatStringsSep ", " structure.vocabularyMap.${value.coordinate}.members
      }"
    else if value.kind == "RecordRef" then
      "record ${value.id}"
    else if value.kind == "NativeDomain" then
      "${value.id} (${valueText value.wire}): ${value.description}"
    else if value.kind == "Integer" then
      "${lib.optionalString value.nonzero "nonzero "}${
        if value.signed then "i" else "u"
      }${toString value.bits}"
    else if value.kind == "List" then
      "${lib.optionalString value.unique "unique "}list of (${valueText value.element})"
    else if value.kind == "Map" then
      "string-keyed map of (${valueText value.value})"
    else if value.kind == "OpenJson" then
      "open JSON: ${value.description}"
    else
      value.kind;
  recordText = record: ''
    ## record ${record.id}

    ${record.description}

    Decoder: ${record.decoder}. Native validation and lowering retain cross-field, graph and host checks.

    ${lib.concatMapStringsSep "\n\n" (field: ''
      ### ${field.name}
      ${field.description}

      Domain: ${valueText field.value}

      Decode: ${field.decode.kind}${
        lib.optionalString (field.decode.kind == "Default") " ${builtins.toJSON field.decode.literal}"
      }.
      Rust emission: ${field.rustEncode}. Nix emission: ${field.nixEncode}.
    '') record.fields}

  '';
  # Authored ATX sections include descendants and end at a peer or ancestor.
  # Fenced examples cannot introduce section boundaries.
  extractSection =
    document: heading:
    let
      scan =
        lib.foldl'
          (
            state: line:
            let
              # Four-space indentation is code, not an opening/closing fence.
              fence = builtins.match " {0,3}(`{3,}|~{3,})(.*)" line;
              marker = if fence == null then null else builtins.elemAt fence 0;
              tail = if fence == null then "" else builtins.elemAt fence 1;
              opens =
                state.fence == null && marker != null && (lib.hasPrefix "~" marker || !(lib.hasInfix "`" tail));
              closes =
                state.fence != null
                && marker != null
                && lib.hasPrefix state.fence marker
                && builtins.match "[ \t]*" tail != null;
              header =
                if state.fence == null && marker == null then builtins.match "(#{1,6}) +(.+)" line else null;
              selected = header != null && line == heading;
              level = if header == null then 0 else builtins.stringLength (builtins.head header);
              active = selected || (state.active && (header == null || level > state.level));
            in
            {
              fence =
                if closes then
                  null
                else if opens then
                  marker
                else
                  state.fence;
              count = state.count + (if selected then 1 else 0);
              level = if selected then level else state.level;
              inherit active;
              lines = state.lines ++ lib.optional active line;
            }
          )
          {
            fence = null;
            count = 0;
            level = 0;
            active = false;
            lines = [ ];
          }
          (lib.splitString "\n" document);
    in
    if scan.count != 1 then
      throw "Nixfied docs: expected exactly one authored heading ${heading}"
    else
      lib.concatStringsSep "\n" scan.lines;
  composeFragments =
    fragments:
    let
      valid =
        fragment:
        builtins.isAttrs fragment
        &&
          builtins.attrNames fragment == [
            "file"
            "heading"
          ]
        && (
          builtins.isPath fragment.file
          || (builtins.isString fragment.file && lib.hasPrefix "/" fragment.file)
        )
        && builtins.isString fragment.heading
        && builtins.match "#{1,6} .+" fragment.heading != null;
      identities = map (
        fragment:
        builtins.toJSON [
          (toString fragment.file)
          fragment.heading
        ]
      ) fragments;
      prose = lib.concatMapStringsSep "\n" (
        fragment: extractSection (builtins.readFile fragment.file) fragment.heading
      ) fragments;
    in
    if !(builtins.isList fragments) || fragments == [ ] || !(lib.all valid fragments) then
      throw "Nixfied docs: expected nonempty file/heading fragments"
    else if lib.unique identities != identities then
      throw "Nixfied docs: duplicate authored fragment"
    else
      builtins.deepSeq prose {
        inherit prose;
        fragments = map (fragment: {
          document = builtins.baseNameOf fragment.file;
          inherit (fragment) heading;
        }) fragments;
      };
  documentPaths = lib.unique (
    lib.concatMap (topic: map (fragment: toString fragment.file) topic.fragments) (
      builtins.attrValues topics
    )
  );
  documentNames = map builtins.baseNameOf documentPaths;
  documents =
    if lib.unique documentNames != documentNames then
      throw "Nixfied docs: ambiguous document basename"
    else
      builtins.listToAttrs (
        map (path: {
          name = builtins.baseNameOf path;
          value = builtins.readFile path;
        }) documentPaths
      );
  optionText = entry: ''
    ## ${entry.name}

    Type: ${entry.type}

    ${entry.description}
    ${lib.optionalString (entry ? default) ''

      Default:
      ```nix
      ${entry.default.text}
      ```
    ''}
    ${lib.optionalString (entry ? example) ''

      Example:
      ```nix
      ${entry.example.text}
      ```
    ''}

  '';
  publicationText = entry: ''
    ## ${entry.kind} ${entry.id}

    ${entry.description}

    Usage:
    ```nix
    ${entry.usage}
    ```

    ${builtins.concatStringsSep "\n\n" (
      map (field: "${field}: ${entry.${field}}") (
        builtins.filter (field: entry ? ${field}) [
          "input"
          "result"
          "contributes"
          "valueDomain"
          "provider"
          "effects"
          "artifact"
        ]
      )
    )}

  '';
  ref = kind: id: { inherit kind id; };
  # Native option loc, rather than a reconstructed dotted path, owns identity.
  normalizeReference =
    reference:
    if reference.kind == "option" then
      let
        entry = lib.findFirst (option: option.loc == reference.path) null options;
      in
      if entry == null then throw "Nixfied docs: unknown option reference" else ref "option" entry.name
    else
      reference;
  valueReferences =
    value:
    if value.kind == "RecordRef" then
      [ (ref "record" value.id) ]
    else if value.kind == "Enum" then
      [ (ref "vocabulary" value.coordinate) ]
    else if value.kind == "NativeDomain" then
      valueReferences value.wire
    else if value.kind == "List" then
      valueReferences value.element
    else if value.kind == "Map" then
      valueReferences value.value
    else
      [ ];
  argumentDomain =
    domain:
    if domain.kind == "Enum" then
      lib.concatStringsSep " | " structure.vocabularyMap.${domain.coordinate}.members
    else
      domain.kind + lib.optionalString (domain.kind == "Unsigned") (toString domain.bits);
  publicationSummary =
    entry:
    lib.concatStringsSep "\n\n" (
      [
        entry.description
        "Usage: `${entry.usage}`"
      ]
      ++ map (field: "${field}: ${entry.${field}}") (
        builtins.filter (field: entry ? ${field}) [
          "input"
          "result"
          "contributes"
          "valueDomain"
          "provider"
          "effects"
          "artifact"
        ]
      )
    );
  optionEntries = map (entry: {
    kind = "option";
    id = entry.name;
    inherit (entry) name loc description;
    text = optionText entry;
    summary =
      "Type: ${entry.type}\n\n${entry.description}"
      + lib.optionalString (entry ? default) (
        if lib.hasInfix "\n" entry.default.text then
          "\n\nDefault:\n```nix\n${entry.default.text}\n```"
        else
          "\n\nDefault: `${entry.default.text}`"
      );
    references = [ ];
  }) options;
  apiEntries =
    map (entry: {
      inherit (entry) kind id description;
      text = publicationText entry;
      summary = publicationSummary entry;
      references = map normalizeReference entry.references;
    }) publications
    ++ map (command: {
      kind = "command";
      id = command.name;
      inherit (command) description;
      text = commandText command;
      summary =
        command.description
        + "\n\nVisible arguments:\n\n"
        + lib.concatMapStringsSep "\n" (
          argument:
          "- `${argument.token}`: ${argument.help.text} (${argumentDomain argument.valueDomain})"
          + lib.optionalString (
            argument.initialValue.kind != "Absent"
          ) "; initial value: `${builtins.toJSON argument.initialValue.value}`"
        ) (builtins.filter (argument: argument.help.kind == "Visible") command.arguments);
      references =
        command.references
        ++ lib.concatMap (
          argument:
          lib.optional (argument.valueDomain.kind == "Enum") (
            ref "vocabulary" argument.valueDomain.coordinate
          )
        ) command.arguments;
    }) syntax.commands
    ++ map (record: {
      kind = "record";
      inherit (record) id description;
      text = recordText record;
      summary = record.description;
      references = lib.concatMap (field: valueReferences field.value) record.fields;
    }) structure.records
    ++ lib.mapAttrsToList (id: annotation: {
      kind = "error";
      inherit id;
      inherit (annotation) description;
      inherit (annotation) contextTopic;
      summary = annotation.description;
      text = ''
        ## error ${id}

        ${annotation.description}

        Related topic: `docs topic ${annotation.contextTopic}`.
        Shape: `docs api record output-schema/runtime-error`.
        Native constructors select the exit class; numeric process statuses and failure precedence remain native.
        See `docs topic errors` for nested and open diagnostic fields.
      '';
      references = [
        (ref "topic" annotation.contextTopic)
        (ref "record" "output-schema/runtime-error")
      ];
    }) structure.vocabularyMap."error-code".annotations;
  vocabularyEntries = map (vocabulary: {
    kind = "vocabulary";
    id = vocabulary.coordinate;
    inherit (vocabulary) description;
    summary = vocabulary.description + "\n\nMembers: ${lib.concatStringsSep ", " vocabulary.members}.";
    text = "## ${vocabulary.coordinate}\n\n${vocabulary.description}\n\nMembers: ${lib.concatStringsSep ", " vocabulary.members}.\n";
    references = [ ];
  }) structure.vocabularies;
  entries = optionEntries ++ apiEntries ++ vocabularyEntries;
  navigation = import ./navigation.nix { inherit lib; } { inherit entries topics; };
  entryMap = builtins.listToAttrs (
    map (entry: {
      name = navigation.key entry;
      value = entry;
    }) entries
  );
  query =
    target:
    if target.kind == "topic" then
      "docs topic ${target.id}"
    else if target.kind == "option" then
      "docs option ${lib.escapeShellArg target.id}"
    else if target.kind == "vocabulary" then
      null
    else
      "docs api ${target.kind} ${target.id}";
  referenceLine =
    target:
    if target.kind == "vocabulary" then
      "- Domain ${target.id}: ${lib.concatStringsSep ", " structure.vocabularyMap.${target.id}.members}."
    else
      "- See ${target.kind} ${target.id}. `${query target}`";
  referenceList =
    title: targets:
    let
      errors = builtins.filter (target: target.kind == "error") targets;
      other = builtins.filter (target: target.kind != "error") targets;
    in
    lib.optionalString (targets != [ ]) (
      "\n\n### ${title}\n\n"
      + lib.concatMapStringsSep "\n" referenceLine other
      + lib.optionalString (errors != [ ]) (
        "\n\nError codes (query with `docs api error <code>`):\n\n"
        + lib.concatMapStringsSep "\n" (entry: "- `${entry.id}`") errors
      )
      + "\n"
    );
  withNavigation =
    entry:
    entry
    // {
      references = navigation.references entry;
      backlinks = navigation.backlinks entry;
      text =
        entry.text
        + referenceList "Related topics" (
          builtins.filter (r: r.kind == "topic") (navigation.references entry)
        )
        + referenceList "References" (
          builtins.filter (r: r.kind != "topic" && !(entry.kind == "record" && r.kind == "vocabulary")) (
            navigation.references entry
          )
        )
        + referenceList "Referenced by" (navigation.backlinks entry);
    };
  topicText =
    name: topic:
    let
      target = ref "topic" name;
      members = navigation.members name;
      composed = composeFragments topic.fragments;
      inherit (composed) prose fragments;
      related = lib.unique (
        navigation.references target
        ++ builtins.filter (entry: entry.kind == "topic") (navigation.backlinks target)
      );
      errors = builtins.filter (entry: entry.kind == "error") members;
      definitions = builtins.filter (entry: entry.kind != "error") members;
      errorList = lib.optionalString (errors != [ ]) (
        "### Error codes and related topics\n\n"
        + lib.concatMapStringsSep "\n\n" (
          member:
          let
            entry = entryMap.${navigation.key member};
          in
          "- `${entry.id}`\n\n  "
          + lib.replaceStrings [ "\n" ] [ "\n  " ] entry.description
          + "\n\n  Related topic: "
          + (if entry.contextTopic == name then "This topic" else "`docs topic ${entry.contextTopic}`")
          + "\n\n  Details: `${query member}`"
        ) errors
        + "\n"
      );
      cards = lib.concatMapStringsSep "\n\n" (
        member:
        let
          entry = entryMap.${navigation.key member};
        in
        "### ${entry.kind} ${entry.id}\n\n${entry.summary}"
        + lib.optionalString (query member != null) "\n\nDetails: `${query member}`"
      ) definitions;
      references =
        if members != [ ] && (builtins.head members).kind == "error" then
          errorList + "\n" + cards
        else
          cards + lib.optionalString (errors != [ ]) ("\n\n" + errorList);
    in
    {
      section = lib.concatMapStringsSep " / " (
        fragment: "${fragment.document}: ${fragment.heading}"
      ) fragments;
      inherit
        prose
        fragments
        members
        related
        ;
      text =
        "Topic: ${name}\n\n"
        + prose
        + referenceList "Related topics" related
        + lib.optionalString (members != [ ]) ("\n## Related definitions\n\n" + references + "\n");
    };
  reference = {
    inherit source documents system;
    vocabularies = map withNavigation vocabularyEntries;
    topics = builtins.mapAttrs topicText topics;
    options = map withNavigation optionEntries;
    api = map withNavigation apiEntries;
  };
  # Display strings can mention store paths. They must not retain the products
  # they describe. Configured defaults and executable bindings keep context.
  serialized = builtins.unsafeDiscardStringContext (builtins.toJSON reference);
  index = pkgs.writeText "nixfied-reference.json" serialized;
  content = pkgs.runCommand "nixfied-reference" { nativeBuildInputs = [ pkgs.jq ]; } ''
    mkdir -p "$out/share/nixfied/reference"
    cp ${index} "$out/share/nixfied/reference/index.json"
    jq -r '
      "# Nixfied authoring and API reference\n",
      "Supplying framework source:\n", (.source | to_entries[] | select(.value != null) | "\(.key): \(.value)"),
      "\n## Topics\n", (.topics | to_entries[] | "- \(.key): \(.value.section)"),
      "\n# Options\n", (.options | sort_by(.name)[] | .text),
      "\n# Public API\n", (.api | sort_by(.kind, .id)[] | .text),
      "\n# Wire vocabularies\n", (.vocabularies[].text),
      "\n# Native guides and contracts\n", (.documents | to_entries[] | .value)
    ' ${index} > "$out/share/nixfied/reference/API.md"
  '';
  command = pkgs.writeShellApplication {
    name = "nixfied-docs";
    runtimeInputs = [ pkgs.jq ];
    inheritPath = false;
    text = ''
      index=${content}/share/nixfied/reference/index.json
      # Native read-only dispatcher over private packaged presentation data.
      usage() {
        printf '%s\n' "Usage: docs
             docs -h | --help
             docs options [prefix]
             docs option <exact-path>
             docs topic <name>
             docs api [kind]
             docs api <kind> <exact-id>
             docs source

      Quote keyed option paths, for example:
        docs option 'nixfied.services.<name>.stateRefs'
        docs topic state
        docs topic placeholders
        docs api function library/compileManifest"
      }
      fail() {
        printf 'docs: %s; use docs --help for query usage.\n' "$1" >&2
        exit 2
      }
      source_info() {
        jq -r '.source | to_entries[] | select(.value != null) | "\(.key): \(.value)"' "$index"
      }
      for argument in "$@"; do
        [[ -n "$argument" ]] || fail 'empty queries are not supported'
      done
      if [[ $# == 0 ]]; then
        printf 'Nixfied authoring and API reference\n\nSupplying framework source:\n'
        source_info
        printf '\nTopics:\n'
        jq -r '.topics | to_entries[] | "  \(.key): \(.value.section)"' "$index"
        printf '\n'
        usage
        exit 0
      fi
      case "$1" in
        -h|--help)
          [[ $# == 1 ]] || fail 'help accepts no operands'
          usage
          ;;
        source)
          [[ $# == 1 ]] || fail 'source accepts no operands'
          source_info
          ;;
        options)
          [[ $# -le 2 ]] || fail 'options accepts at most one namespace prefix'
          prefix="''${2-}"
          if ! jq -er --arg prefix "$prefix" '
            [.options[].name | select($prefix == "" or . == $prefix or startswith($prefix + "."))]
            | sort | if length == 0 then empty else .[] end
          ' "$index"; then
            fail "unknown option namespace: $prefix"
          fi
          ;;
        option)
          [[ $# == 2 ]] || fail 'option requires one exact canonical path'
          if ! jq -er --arg name "$2" '.options[] | select(.name == $name) | .text' "$index"; then
            fail "unknown option: $2 (use docs options to list canonical paths)"
          fi
          ;;
        topic)
          [[ $# == 2 ]] || fail 'topic requires one exact name'
          if ! jq -er --arg name "$2" '
            if .topics[$name] then .topics[$name] as $topic
            | $topic.text
            else empty end
          ' "$index"; then
            fail "unknown topic: $2 (use docs to list topics)"
          fi
          ;;
        api)
          [[ $# -le 3 ]] || fail 'api accepts a kind and an optional exact ID'
          if [[ $# == 1 ]]; then
            jq -r '[.api[].kind] | unique[]' "$index"
          elif [[ $# == 2 ]]; then
            if ! jq -er --arg kind "$2" '[.api[] | select(.kind == $kind) | .id] | sort | if length == 0 then empty else .[] end' "$index"; then
              fail "unknown API kind: $2 (use docs api to list kinds)"
            fi
          elif ! jq -er --arg kind "$2" --arg id "$3" '.api[] | select(.kind == $kind and .id == $id) | .text' "$index"; then
            fail "unknown API entry: $2 $3 (use docs api <kind> to list IDs)"
          fi
          ;;
        *) fail "unknown query: $1" ;;
      esac
    '';
  };
in
pkgs.symlinkJoin {
  name = "nixfied-docs";
  paths = [
    content
    command
  ];
  passthru = { inherit serialized extractSection composeFragments; };
}
