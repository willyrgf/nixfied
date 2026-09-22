# One checked structural representation for model and native output boundaries.
{ lib }:
{
  inventory,
  records,
  vocabularies,
  contextTopics ? [ ],
}:
let
  fail = message: throw "Nixfied structure: ${message}";
  require =
    condition: message: value:
    if condition then value else fail message;
  nonBlank = value: builtins.isString value && builtins.match "[[:space:]]*" value == null;
  exact =
    allowed: attrs:
    builtins.isAttrs attrs
    && builtins.removeAttrs attrs allowed == { }
    && builtins.all (name: attrs ? ${name}) allowed;
  ident =
    name:
    builtins.isString name
    && builtins.match "[A-Za-z_][A-Za-z0-9_]*" name != null
    && !(builtins.elem name [
      "_"
      "as"
      "async"
      "await"
      "break"
      "const"
      "continue"
      "crate"
      "dyn"
      "else"
      "enum"
      "extern"
      "false"
      "fn"
      "for"
      "if"
      "impl"
      "in"
      "let"
      "loop"
      "match"
      "mod"
      "move"
      "mut"
      "pub"
      "ref"
      "return"
      "self"
      "Self"
      "static"
      "struct"
      "super"
      "trait"
      "true"
      "type"
      "unsafe"
      "use"
      "where"
      "while"
      "abstract"
      "become"
      "box"
      "do"
      "final"
      "gen"
      "macro"
      "override"
      "priv"
      "typeof"
      "unsized"
      "virtual"
      "yield"
      "try"
    ]);
  nativePath =
    path:
    builtins.isList path
    && path != [ ]
    && (
      ident (builtins.head path)
      || builtins.elem (builtins.head path) [
        "crate"
        "self"
        "super"
      ]
    )
    && builtins.all ident (builtins.tail path);
  snake =
    name:
    lib.concatMapStrings (
      char: if builtins.match "[A-Z]" char != null then "_${lib.toLower char}" else char
    ) (lib.stringToCharacters name);
  variant =
    member:
    lib.concatMapStrings (
      word: lib.toUpper (builtins.substring 0 1 word) + builtins.substring 1 (-1) (lib.toLower word)
    ) (lib.splitString "-" (lib.replaceStrings [ "_" ] [ "-" ] member));
  identity =
    value:
    if (value.kind or null) == "Inventory" then
      require (
        exact [ "kind" "coordinate" ] value
        && nonBlank (value.coordinate or null)
        && (lib.hasPrefix "primitive " value.coordinate || lib.hasPrefix "output-schema " value.coordinate)
        && inventory ? ${value.coordinate}
      ) "invalid inventory record identity" (lib.replaceStrings [ " " ] [ "/" ] value.coordinate)
    else if (value.kind or null) == "Local" then
      require (
        exact [ "kind" "name" ] value
        && ident (value.name or null)
        && !(inventory ? ${"primitive " + value.name})
        && !(inventory ? ${"output-schema " + value.name})
      ) "invalid local record identity" "local/${value.name}"
    else
      fail "unknown record identity form";
  visibility =
    value:
    builtins.elem value [
      "pub"
      "pub(crate)"
      "private"
    ];
  derivesValid =
    derives:
    builtins.isList derives
    && lib.unique derives == derives
    && builtins.all (
      derive:
      builtins.elem derive [
        "Debug"
        "Clone"
        "Copy"
        "PartialEq"
        "Eq"
      ]
    ) derives;
  binding =
    rust:
    require (
      exact [ "file" "name" "emission" "visibility" "derives" ] rust
      && nonBlank (rust.file or null)
      && lib.hasSuffix ".rs" rust.file
      && !(lib.hasPrefix "/" rust.file)
      && !(builtins.elem ".." (lib.splitString "/" rust.file))
      && ident (rust.name or null)
      && visibility (rust.visibility or null)
      && derivesValid (rust.derives or null)
      && builtins.elem (rust.emission or null) [
        "Owned"
        "Borrowed"
        "MemberNamesOnly"
      ]
    ) "invalid Rust record binding" rust;
  normalizeVocabulary =
    declaration:
    require
      (
        exact (
          [
            "coordinate"
            "description"
            "rust"
            "decoder"
          ]
          ++ lib.optional ((declaration.coordinate or null) == "error-code") "annotations"
        ) declaration
        && nonBlank (declaration.description or null)
        && builtins.elem declaration.decoder [
          "Closed"
          "NoDecoder"
        ]
        && inventory ? ${declaration.coordinate or ""}
        && (
          lib.hasPrefix "enum " declaration.coordinate
          || declaration.coordinate == "signal"
          || lib.hasPrefix "status " declaration.coordinate
          || builtins.elem declaration.coordinate [
            "error-code"
            "exit-class"
            "run-output-mode"
          ]
        )
      )
      "invalid vocabulary declaration"
      (
        let
          rust = binding declaration.rust;
          members = inventory.${declaration.coordinate};
          variants = map variant members;
          annotations = declaration.annotations or { };
          status = lib.hasPrefix "status " declaration.coordinate;
        in
        require (
          rust.emission == "Owned"
          && lib.unique variants == variants
          && builtins.all ident variants
          && (
            !status
            || (
              declaration.decoder == "NoDecoder"
              && rust.visibility == "pub"
              &&
                builtins.sort builtins.lessThan rust.derives == [
                  "Clone"
                  "Copy"
                  "Debug"
                  "Eq"
                  "PartialEq"
                ]
            )
          )
          && (
            declaration.coordinate != "error-code"
            || (
              builtins.isAttrs annotations
              && builtins.attrNames annotations == builtins.sort builtins.lessThan members
              && builtins.all (
                entry:
                exact [ "description" "contextTopic" ] entry
                && nonBlank entry.description
                && builtins.elem entry.contextTopic contextTopics
              ) (builtins.attrValues annotations)
            )
          )
        ) "invalid or colliding vocabulary variants" (declaration // { inherit rust members variants; })
      );
  vocabularyList = map normalizeVocabulary vocabularies;
  vocabularyNames = map (entry: entry.coordinate) vocabularyList;
  vocabularyMap = builtins.listToAttrs (
    map (entry: {
      name = entry.coordinate;
      value = entry;
    }) vocabularyList
  );
  normalizeValue =
    value:
    let
      kind = value.kind or "";
      fields =
        {
          Text = [ ];
          Boolean = [ ];
          Integer = [
            "signed"
            "bits"
            "nonzero"
          ];
          Enum = [ "coordinate" ];
          List = [
            "element"
            "unique"
          ];
          Map = [ "value" ];
          RecordRef = [ "identity" ];
          NativeDomain = [
            "id"
            "wire"
            "description"
            "producerCheck"
            "rustPath"
          ];
          OpenJson = [ "description" ];
        }
        .${kind} or (fail "unsupported value form");
      keysValid = exact ([ "kind" ] ++ fields) value && builtins.all (field: value ? ${field}) fields;
    in
    require keysValid "missing or unknown value attributes" (
      if kind == "Integer" then
        require (
          builtins.isBool value.signed
          && builtins.isBool value.nonzero
          && builtins.elem value.bits (
            if value.signed then
              [
                32
                64
              ]
            else
              [
                16
                32
                64
              ]
          )
        ) "unsupported integer domain" value
      else if kind == "Enum" then
        require (
          nonBlank value.coordinate && vocabularyMap ? ${value.coordinate}
        ) "unbound inventory vocabulary" value
      else if kind == "RecordRef" then
        value // { id = identity value.identity; }
      else if kind == "List" then
        require (builtins.isBool value.unique) "list uniqueness must be explicit" (
          value // { element = normalizeValue value.element; }
        )
      else if kind == "Map" then
        value // { value = normalizeValue value.value; }
      else if kind == "NativeDomain" then
        let
          wire = normalizeValue value.wire;
        in
        require
          (
            ident value.id
            && nonBlank value.description
            && builtins.elem wire.kind [
              "Text"
              "Boolean"
              "Integer"
              "Enum"
            ]
            && (value.producerCheck == null || lib.isFunction value.producerCheck)
            && nativePath value.rustPath
          )
          "native domains require a scalar wire form and checked native type path"
          (value // { inherit wire; })
      else if kind == "OpenJson" then
        require (nonBlank value.description) "open JSON needs an explanation" value
      else
        value
    );
  children =
    value:
    if value.kind == "List" then
      [ value.element ]
    else if value.kind == "Map" then
      [ value.value ]
    else if value.kind == "NativeDomain" then
      [ value.wire ]
    else
      [ ];
  walk = f: value: f value ++ lib.concatMap (walk f) (children value);
  refs = value: walk (v: lib.optional (v.kind == "RecordRef") v.id) value;
  collection =
    value:
    builtins.elem value.kind [
      "List"
      "Map"
    ];
  emptyValue = value: if value.kind == "List" then [ ] else { };
  acceptsMissing =
    field:
    builtins.elem field.decode.kind [
      "Optional"
      "Default"
    ];
  normalizeField =
    record: field:
    let
      value = normalizeValue field.value;
      presence = field.presence;
      kind = presence.kind or "";
      policyOf = decode: rustEncode: { inherit decode rustEncode; };
      required = {
        kind = "Required";
      };
      optional = {
        kind = "Optional";
      };
      empty = {
        kind = "Default";
        literal = emptyValue value;
      };
      policies = {
        Required = policyOf required "Present";
        Optional = policyOf optional "Present";
        OptionalOmitted = policyOf optional "OmitAbsent";
        Empty = policyOf empty "Present";
        EmptyOmitted = policyOf empty "OmitEmpty";
        EnumDefault = policyOf {
          kind = "Default";
          literal = presence.member;
        } "Present";
        OmitEmpty = policyOf required "OmitEmpty";
      };
      policy = policies.${kind} or (fail "unsupported field presence");
      rust = {
        name = snake field.name;
        visibility = record.rust.visibility;
        storage = "Direct";
      }
      // field.rust;
      normalized =
        field
        // policy
        // {
          inherit value rust;
          nixEncode = field.nixEncode or "NotProduced";
        };
    in
    require (
      exact (
        [
          "name"
          "description"
          "value"
          "presence"
          "rust"
        ]
        ++ lib.optional record.produced "nixEncode"
      ) field
      && nonBlank field.name
      && nonBlank field.description
      && exact ([ "kind" ] ++ lib.optional (kind == "EnumDefault") "member") presence
      && (
        !(builtins.elem kind [
          "Empty"
          "EmptyOmitted"
          "OmitEmpty"
        ])
        || collection value
      )
      && (
        kind != "EnumDefault"
        || (value.kind == "Enum" && builtins.elem presence.member vocabularyMap.${value.coordinate}.members)
      )
      && (kind != "OmitEmpty" || record.decoder == "NoDecoder")
      && exact [ "name" "visibility" "storage" ] rust
      && ident rust.name
      && visibility rust.visibility
      && builtins.elem rust.storage [
        "Direct"
        "Box"
      ]
      && (record.rust.emission == "Owned" || rust.storage == "Direct")
      && (
        !record.produced
        || (
          builtins.elem field.nixEncode [
            "RequiredPresent"
            "RequiredOmitAbsent"
            "RequiredOmitEmpty"
            "PreserveSupplied"
          ]
          && (field.nixEncode != "RequiredOmitAbsent" || policy.decode.kind == "Optional")
          && (
            field.nixEncode != "RequiredOmitEmpty"
            || builtins.elem kind [
              "Empty"
              "EmptyOmitted"
            ]
          )
          && (field.nixEncode != "PreserveSupplied" || acceptsMissing normalized)
        )
      )
    ) "invalid field or presence in ${record.id}.${field.name or "?"}" normalized;
  normalizeRecord =
    declaration:
    let
      id = identity declaration.identity;
      rust = binding declaration.rust;
      record = declaration // {
        inherit id rust;
        produced = declaration.producer == "Nix";
      };
      fields = map (normalizeField record) declaration.fields;
      names = map (field: field.name) fields;
      rustNames = map (field: field.rust.name) fields;
      members =
        if declaration.identity.kind == "Inventory" then
          map (lib.removeSuffix "?") inventory.${declaration.identity.coordinate}
        else
          names;
    in
    require
      (
        exact [ "identity" "description" "decoder" "producer" "fields" "rust" ] declaration
        && builtins.elem declaration.producer [
          "Nix"
          "None"
        ]
        && nonBlank (declaration.description or null)
        && builtins.isList declaration.fields
        && fields != [ ]
        && lib.unique names == names
        && lib.unique rustNames == rustNames
        && builtins.sort builtins.lessThan names == builtins.sort builtins.lessThan members
        && builtins.elem declaration.decoder [
          "RejectUnknown"
          "IgnoreUnknown"
          "NoDecoder"
        ]
        && (rust.emission == "Owned" || declaration.decoder == "NoDecoder")
        && (rust.emission != "MemberNamesOnly" || builtins.length fields == 1)
      )
      "invalid record or inventory coverage"
      (
        record
        // {
          inherit fields;
        }
      );
  recordList = map normalizeRecord records;
  recordIds = map (record: record.id) recordList;
  recordMap = builtins.listToAttrs (
    map (record: {
      name = record.id;
      value = record;
    }) recordList
  );
  isJson =
    value:
    value == null
    || builtins.isString value
    || builtins.isBool value
    || builtins.isInt value
    || builtins.isFloat value
    || (builtins.isList value && builtins.all isJson value)
    || (builtins.isAttrs value && builtins.all isJson (builtins.attrValues value));
  matches =
    value: input:
    if value.kind == "Text" then
      builtins.isString input
    else if value.kind == "Boolean" then
      builtins.isBool input
    else if value.kind == "Integer" then
      (
        if value.bits == 64 then
          (if value.signed then lib.types.int else lib.types.ints.unsigned)
        else
          lib.types.ints.${"${if value.signed then "s" else "u"}${toString value.bits}"}
      ).check
        input
      && (!value.nonzero || input != 0)
    else if value.kind == "Enum" then
      builtins.isString input && builtins.elem input vocabularyMap.${value.coordinate}.members
    else if value.kind == "List" then
      builtins.isList input
      && builtins.all (matches value.element) input
      && (!value.unique || lib.unique input == input)
    else if value.kind == "Map" then
      builtins.isAttrs input && builtins.all (matches value.value) (builtins.attrValues input)
    else if value.kind == "RecordRef" then
      recordMatches recordMap.${value.id} input
    else if value.kind == "NativeDomain" then
      matches value.wire input && (value.producerCheck == null || value.producerCheck input)
    else
      isJson input;
  fieldMatches =
    field: input: (input == null && field.decode.kind == "Optional") || matches field.value input;
  recordMatches =
    record: input:
    builtins.isAttrs input
    && record.produced
    && builtins.removeAttrs input (map (field: field.name) record.fields) == { }
    && builtins.all (
      field:
      if input ? ${field.name} then
        fieldMatches field input.${field.name}
        && (field.nixEncode != "RequiredOmitAbsent" || input.${field.name} != null)
        && (field.nixEncode != "RequiredOmitEmpty" || input.${field.name} != emptyValue field.value)
      else
        field.nixEncode != "RequiredPresent"
    ) record.fields;
  checkGraph =
    trail: record:
    require (!(builtins.elem record.id trail)) "recursive record graph" (
      builtins.all (
        field:
        builtins.all (
          id:
          require (recordMap ? ${id}) "dangling record reference ${id}" (
            let
              target = recordMap.${id};
            in
            require (
              target.rust.emission != "MemberNamesOnly"
              && (record.rust.emission != "Owned" || target.rust.emission == "Owned")
            ) "invalid record storage reference" (checkGraph (trail ++ [ record.id ]) target)
          )
        ) (refs field.value)
      ) record.fields
    );
  checkBorrowed =
    record:
    record.rust.emission != "Borrowed"
    || builtins.all (
      field:
      builtins.all (
        v: !collection v || builtins.all (id: recordMap.${id}.rust.emission == "Owned") (refs v)
      ) (walk (v: [ v ]) field.value)
    ) record.fields;
  # Functions in native domain predicates are deliberately not deep-forced.
  metadata = map (record: {
    inherit (record)
      id
      description
      decoder
      rust
      produced
      ;
    fields = map (
      field:
      builtins.removeAttrs field [ "value" ]
      // {
        value = map (
          v:
          builtins.removeAttrs v [
            "producerCheck"
            "wire"
            "element"
            "value"
          ]
        ) (walk (v: [ v ]) field.value);
      }
    ) record.fields;
  }) recordList;
  bindingNames = map (r: builtins.toJSON { inherit (r.rust) file name; }) (
    recordList ++ vocabularyList
  );
  valid =
    require
      (
        lib.unique recordIds == recordIds
        && lib.unique vocabularyNames == vocabularyNames
        && lib.unique bindingNames == bindingNames
      )
      "duplicate structural identity or Rust binding"
      (
        builtins.deepSeq metadata (
          builtins.deepSeq vocabularyList (
            require (
              builtins.all (checkGraph [ ]) recordList && builtins.all checkBorrowed recordList
            ) "unsupported borrowed collection projection" true
          )
        )
      );
  construct =
    record: input:
    require record.produced "record has no Nix producer" (
      require
        (
          builtins.isAttrs input && builtins.removeAttrs input (map (field: field.name) record.fields) == { }
        )
        "unknown constructor fields for ${record.id}"
        (
          require
            (builtins.all (field: input ? ${field.name} || field.nixEncode == "PreserveSupplied") record.fields)
            "missing constructor input for ${record.id}"
            (
              builtins.listToAttrs (
                lib.concatMap (
                  field:
                  let
                    supplied = input ? ${field.name};
                    value = input.${field.name};
                    checked =
                      require (fieldMatches field value) "invalid constructor value for ${record.id}.${field.name}"
                        value;
                    omit =
                      !supplied
                      || (field.nixEncode == "RequiredOmitAbsent" && checked == null)
                      || (field.nixEncode == "RequiredOmitEmpty" && checked == emptyValue field.value);
                  in
                  lib.optional (!omit) {
                    name = field.name;
                    value = checked;
                  }
                ) record.fields
              )
            )
        )
    );
in
builtins.seq valid {
  records = recordList;
  vocabularies = vocabularyList;
  inherit
    recordMap
    vocabularyMap
    matches
    walk
    snake
    variant
    ;
  constructors = builtins.mapAttrs (_: construct) (
    lib.filterAttrs (_: record: record.produced) recordMap
  );
}
