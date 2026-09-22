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
    (if argument.binding == "Shared" then "Runtime" else structure.variant command.name)
    + structure.variant (structure.snake argument.id)
    + "Value";
  prefix =
    command: argument:
    "${if argument.binding == "Shared" then "RUNTIME" else upper command.name}_${upper argument.id}";
  argumentSymbols =
    command: arg:
    {
      token = prefix command arg;
      initial = "${prefix command arg}_INITIAL";
    }
    // lib.optionalAttrs (builtins.elem arg.valueDomain.kind [
      "Unsigned"
      "Enum"
    ]) { type = typeName command arg; };
  commandSymbols = name: {
    command = "${upper name}_COMMAND";
    help = "${upper name}_HELP";
  };
  vocabularySymbols =
    coordinate:
    let
      v = structure.vocabularyMap.${coordinate};
      prefix = upper v.rust.name;
    in
    {
      choices = "${prefix}_CHOICES";
      members = lib.genAttrs v.members (member: "${prefix}_${upper member}");
    };
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
      (if value.bits == 64 then lib.types.ints.unsigned else lib.types.ints.u32).check literal
    else if value.kind == "Enum" then
      builtins.isString literal
      && builtins.elem literal structure.vocabularyMap.${value.coordinate}.members
    else
      builtins.isString literal;
  argument =
    value:
    require (exact [ "id" "token" "binding" "valueDomain" "initialValue" "help" ] value)
      "invalid argument keys"
      (
        let
          valueDomain = domain value.valueDomain;
          initial = value.initialValue;
          help = value.help;
        in
        require
          (
            builtins.elem value.binding [
              "Shared"
              "Command"
            ]
            && identifier value.id
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
                inherit (value)
                  name
                  description
                  references
                  renderHelp
                  ;
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
  # Shared argument identity deliberately excludes command-specific help text.
  shared = lib.concatMap (c: builtins.filter (a: a.binding == "Shared") c.arguments) commands;
  sharedGroups = lib.groupBy (a: a.id) shared;
  sharedFacts = a: builtins.removeAttrs a [ "help" ];
  argumentsFor =
    names:
    lib.unique (
      lib.concatMap (
        c:
        map (arg: {
          symbols = argumentSymbols c arg;
          facts = sharedFacts arg;
        }) c.arguments
      ) (builtins.filter (c: builtins.elem c.name names) commands)
    );
  generatedNames = [
    "HELP_SHORT"
    "HELP_LONG"
  ]
  ++ lib.concatMap (
    coordinate:
    let
      s = vocabularySymbols coordinate;
    in
    [ s.choices ] ++ builtins.attrValues s.members
  ) enumCoordinates
  ++ lib.concatMap (c: builtins.attrValues (commandSymbols c.name)) commands
  ++ lib.concatMap (a: builtins.attrValues a.symbols) (argumentsFor names);
  valid =
    require
      (
        lib.unique names == names
        && lib.unique generatedNames == generatedNames
        && builtins.all (args: builtins.all (a: sharedFacts a == sharedFacts (builtins.head args)) args) (
          builtins.attrValues sharedGroups
        )
      )
      "conflicting generated names or shared argument facts"
      (builtins.deepSeq (map (c: builtins.removeAttrs c [ "renderHelp" ]) commands) true);
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
    argumentSymbols
    commandSymbols
    vocabularySymbols
    argumentsFor
    upper
    ;
  byName = builtins.listToAttrs (
    map (entry: {
      name = entry.name;
      value = entry;
    }) commands
  );
  help = builtins.listToAttrs (
    map (entry: {
      name = entry.name;
      value =
        let
          rendered = entry.renderHelp (facts entry);
        in
        require (builtins.isString rendered) "renderHelp must return text" rendered;
    }) commands
  );
}
