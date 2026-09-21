# One static presentation builder for root and project docs. Native values never
# cross this boundary: only normalised documentation and source identity do.
{
  lib,
  pkgs,
  system,
  options,
  publications,
  source,
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

    ${lib.concatMapStringsSep "\n" (
      reference: "See ${reference.kind} ${reference.id}."
    ) command.references}
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

    Guidance: `docs topic model`, `docs topic derivation`.
  '';
  topics = import ./topics.nix;
  documentPaths = lib.unique (map (topic: topic.file) (builtins.attrValues topics));
  documents = builtins.listToAttrs (
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

    Guidance: `docs topic authoring`, `docs topic state`, `docs topic placeholders`.
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

    ${builtins.concatStringsSep "\n" (
      map (
        reference:
        if reference.kind == "option" then
          "See option ${lib.showOption reference.path}."
        else
          "See ${reference.kind} ${reference.id}."
      ) entry.references
    )}
  '';
  reference = {
    inherit source documents system;
    vocabularies = map (vocabulary: ''
      ## ${vocabulary.coordinate}

      ${vocabulary.description}

      Members: ${lib.concatStringsSep ", " vocabulary.members}.
    '') structure.vocabularies;
    topics = builtins.mapAttrs (_: topic: {
      document = builtins.baseNameOf topic.file;
      inherit (topic) section;
    }) topics;
    options = map (entry: {
      inherit (entry) name loc;
      text = optionText entry;
    }) options;
    api =
      map (entry: {
        inherit (entry) kind id;
        text = publicationText entry;
      }) publications
      ++ map (command: {
        kind = "command";
        id = command.name;
        text = commandText command;
      }) syntax.commands
      ++ map (record: {
        kind = "record";
        inherit (record) id;
        text = recordText record;
      }) structure.records
      ++ lib.mapAttrsToList (id: annotation: {
        kind = "error";
        inherit id;
        text = ''
          ## error ${id}

          ${annotation.description}

          Recovery: `docs topic ${annotation.recoveryTopic}`.
          Shape: `docs api record output-schema/runtime-error`.
          Native constructors select the exit class; numeric process statuses and failure precedence remain native.
          See `docs topic errors` for nested and open diagnostic fields.
        '';
      }) structure.vocabularyMap."error-code".annotations;
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
      "\n## Topics\n", (.topics | to_entries[] | "- \(.key): \(.value.document) — \(.value.section)"),
      "\n# Options\n", (.options | sort_by(.name)[] | .text),
      "\n# Public API\n", (.api | sort_by(.kind, .id)[] | .text),
      "\n# Wire vocabularies\n", (.vocabularies[]),
      "\n# Native guides and contracts\n", (.documents | to_entries[] | .value)
    ' ${index} > "$out/share/nixfied/reference/API.md"
  '';
  command = pkgs.writeShellApplication {
    name = "nixfied-docs";
    runtimeInputs = [ pkgs.jq ];
    inheritPath = false;
    text = ''
      index=${content}/share/nixfied/reference/index.json
      ${builtins.readFile ./query.sh}
    '';
  };
in
pkgs.symlinkJoin {
  name = "nixfied-docs";
  paths = [
    content
    command
  ];
  passthru = { inherit serialized; };
}
