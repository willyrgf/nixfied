# One checked syntax vocabulary; no parser policies or native product bindings.
{ lib }:
{
  declarations,
  structure,
  inventory,
  topics,
  publications,
}:
let
  require =
    condition: message: value:
    if condition then value else throw "Nixfied syntax: ${message}";
  exact =
    keys: value:
    builtins.isAttrs value && builtins.attrNames value == builtins.sort builtins.lessThan keys;
  nonBlank = value: builtins.isString value && builtins.match "[[:space:]]*" value == null;
  identifier = value: builtins.isString value && builtins.match "[a-z][a-zA-Z0-9]*" value != null;
  upper =
    value:
    lib.toUpper (lib.replaceStrings [ "-" ] [ "_" ] (lib.removePrefix "_" (structure.snake value)));
  typeName =
    command: argument:
    structure.variant command.name + structure.variant (structure.snake argument.id) + "Value";
  prefix = command: argument: "${upper command.name}_${upper argument.id}";
  domain =
    value:
    require
      (
        builtins.isAttrs value
        && builtins.elem (value.kind or null) [
          "Flag"
          "Text"
          "Path"
          "Unsigned"
          "Enum"
        ]
      )
      "unknown value domain"
      (
        require (
          exact (
            [ "kind" ]
            ++ lib.optional (value.kind == "Unsigned") "bits"
            ++ lib.optional (value.kind == "Enum") "coordinate"
          ) value
          && (
            value.kind != "Unsigned"
            || builtins.elem value.bits [
              32
              64
            ]
          )
          && (value.kind != "Enum" || structure.vocabularyMap ? ${value.coordinate})
        ) "invalid value domain" value
      );
  matches =
    value: literal:
    if value.kind == "Flag" then
      builtins.isBool literal
    else if value.kind == "Unsigned" then
      builtins.isInt literal && literal >= 0 && (value.bits == 64 || literal <= 4294967295)
    else if value.kind == "Enum" then
      builtins.isString literal
      && builtins.elem literal structure.vocabularyMap.${value.coordinate}.members
    else
      builtins.isString literal;
  argument =
    value:
    require (exact [ "id" "token" "valueDomain" "initialValue" "help" ] value) "invalid argument keys" (
      let
        valueDomain = domain value.valueDomain;
        initial = value.initialValue;
        help = value.help;
      in
      require
        (
          identifier value.id
          && builtins.isString value.token
          && builtins.match "--[a-z][a-z0-9-]*" value.token != null
        )
        "invalid argument identity"
        (
          require
            (
              builtins.isAttrs initial
              && (
                (initial.kind or null) == "Absent" && exact [ "kind" ] initial
                ||
                  (initial.kind or null) == "Literal"
                  && exact [ "kind" "value" ] initial
                  && matches valueDomain initial.value
              )
            )
            "invalid initial value"
            (
              require (
                builtins.isAttrs help
                && (
                  (help.kind or null) == "Hidden" && exact [ "kind" "explanation" ] help && nonBlank help.explanation
                  ||
                    (help.kind or null) == "Visible"
                    && exact [ "kind" "text" "metavar" ] help
                    && nonBlank help.text
                    && (if valueDomain.kind == "Flag" then help.metavar == null else nonBlank help.metavar)
                )
              ) "invalid help visibility" (value // { inherit valueDomain; })
            )
        )
    );
  reference =
    value:
    exact [ "kind" "id" ] value
    && nonBlank value.id
    && (
      value.kind == "topic" && builtins.elem value.id topics
      || value.kind == "record" && structure.recordMap ? ${value.id}
      || value.kind == "error" && structure.vocabularyMap."error-code".annotations ? ${value.id}
    );
  command =
    value:
    require (exact [ "name" "description" "arguments" "renderHelp" "references" ] value)
      "invalid command keys"
      (
        require
          (
            identifier value.name
            && nonBlank value.description
            && builtins.isList value.arguments
            && builtins.isFunction value.renderHelp
            && builtins.isList value.references
            && value.references != [ ]
            && builtins.all reference value.references
            && lib.any (ref: ref.kind == "topic") value.references
          )
          "invalid command metadata"
          (
            let
              arguments = map argument value.arguments;
              tokens = map (arg: arg.token) arguments ++ helpTokens;
              ids = map (arg: arg.id) arguments;
            in
            require (lib.unique tokens == tokens && lib.unique ids == ids)
              "duplicate argument token or identifier"
              {
                inherit (value) name description references;
                inherit arguments;
                args = builtins.listToAttrs (
                  map (arg: {
                    name = arg.id;
                    value = arg;
                  }) arguments
                );
              }
          )
      );
  helpTokens = require (
    builtins.length inventory.surface-help == 2
    && lib.unique inventory.surface-help == inventory.surface-help
    && builtins.all (
      token: builtins.isString token && builtins.match "-[a-z-]+" token != null
    ) inventory.surface-help
  ) "invalid shared help pair" inventory.surface-help;
  commands = map command declarations;
  names = map (entry: entry.name) commands;
  # Check the actual names emitted by both formats, including suffix collisions.
  enumCoordinates = lib.unique (
    lib.concatMap (
      entry:
      lib.concatMap (
        arg: lib.optional (arg.valueDomain.kind == "Enum") arg.valueDomain.coordinate
      ) entry.arguments
    ) commands
  );
  generatedNames = [
    "HELP_SHORT"
    "HELP_LONG"
  ]
  ++ lib.concatMap (
    coordinate:
    let
      vocabulary = structure.vocabularyMap.${coordinate};
      prefix = upper vocabulary.rust.name;
    in
    [ "${prefix}_CHOICES" ] ++ map (member: "${prefix}_${upper member}") vocabulary.members
  ) enumCoordinates
  ++ lib.concatMap (
    entry:
    [
      "${upper entry.name}_COMMAND"
      "${upper entry.name}_HELP"
    ]
    ++ lib.concatMap (
      arg:
      [
        (prefix entry arg)
        "${prefix entry arg}_INITIAL"
      ]
      ++ lib.optional (builtins.elem arg.valueDomain.kind [
        "Unsigned"
        "Enum"
      ]) (typeName entry arg)
    ) entry.arguments
  ) commands;
  valid = require (
    lib.unique names == names && lib.unique generatedNames == generatedNames
  ) "conflicting generated names" (builtins.deepSeq commands true);
  apps = builtins.listToAttrs (
    map (entry: {
      name = entry.command;
      value = { inherit (entry) name; };
    }) (builtins.filter (entry: entry ? command) publications)
  );
  facts =
    entry:
    entry
    // {
      inherit apps helpTokens;
      vocabularies = builtins.mapAttrs (_: value: value.members) structure.vocabularyMap;
    };
in
builtins.seq valid {
  inherit
    commands
    enumCoordinates
    helpTokens
    prefix
    typeName
    upper
    ;
  byName = builtins.listToAttrs (
    map (entry: {
      name = entry.name;
      value = entry;
    }) commands
  );
  help = builtins.listToAttrs (
    lib.imap0 (index: entry: {
      name = entry.name;
      value =
        let
          rendered = (builtins.elemAt declarations index).renderHelp (facts entry);
        in
        require (builtins.isString rendered) "renderHelp must return text" rendered;
    }) commands
  );
}
