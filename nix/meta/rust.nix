# Deterministic projection of the checked structure into native owner scopes.
{ lib }:
checked:
let
  quote = import ./rust-quote.nix { inherit lib; };
  visibility = value: if value == "private" then "" else "${value} ";
  optional = field: field.decode.kind == "Optional";
  owned =
    value:
    if value.kind == "Text" then
      "String"
    else if value.kind == "Boolean" then
      "bool"
    else if value.kind == "Integer" then
      if value.nonzero then
        "std::num::NonZero${if value.signed then "I" else "U"}${toString value.bits}"
      else
        "${if value.signed then "i" else "u"}${toString value.bits}"
    else if value.kind == "Enum" then
      checked.vocabularyMap.${value.coordinate}.rust.name
    else if value.kind == "NativeDomain" then
      lib.concatStringsSep "::" value.rustPath
    else if value.kind == "OpenJson" then
      "serde_json::Value"
    else if value.kind == "RecordRef" then
      checked.recordMap.${value.id}.rust.name
    else if value.kind == "Map" then
      "BTreeMap<String, ${owned value.value}>"
    else
      "${if value.unique then "UniqueVec" else "Vec"}<${owned value.element}>";
  borrowed =
    value:
    if value.kind == "Text" then
      "&'a str"
    else if value.kind == "List" && !value.unique then
      "&'a [${owned value.element}]"
    else if value.kind == "RecordRef" && checked.recordMap.${value.id}.rust.emission == "Borrowed" then
      "${owned value}<'a>"
    else if
      builtins.elem value.kind [
        "NativeDomain"
        "OpenJson"
        "List"
        "Map"
        "RecordRef"
      ]
    then
      "&'a ${owned value}"
    else
      owned value;
  fieldType =
    record: field:
    let
      base = (if record.rust.emission == "Borrowed" then borrowed else owned) field.value;
      nullable = if optional field then "Option<${base}>" else base;
    in
    if field.rust.storage == "Box" then "Box<${nullable}>" else nullable;
  helperName = record: field: "__default${checked.snake record.rust.name}_${field.rust.name}";
  helper =
    record: field:
    if record.decoder == "NoDecoder" then
      ""
    else if field.presence.kind == "EnumDefault" then
      ''
        fn ${helperName record field}() -> ${fieldType record field} {
            ${
              lib.optionalString (field.rust.storage == "Box") "Box::new("
            }${owned field.value}::${checked.variant field.presence.member}${
              lib.optionalString (field.rust.storage == "Box") ")"
            }
        }
      ''
    else if field.decode.kind == "Required" && field.value.kind == "OpenJson" then
      ''
        fn ${helperName record field}<'de, D: serde::Deserializer<'de>>(deserializer: D) -> Result<${fieldType record field}, D::Error> {
            serde::Deserialize::deserialize(deserializer)
        }
      ''
    else
      "";
  attributes =
    record: field:
    let
      decode = lib.optionals (record.decoder != "NoDecoder") (
        if field.presence.kind == "EnumDefault" then
          [ "default = ${quote (helperName record field)}" ]
        else if
          builtins.elem field.decode.kind [
            "Optional"
            "Default"
          ]
        then
          [ "default" ]
        else if field.decode.kind == "Required" && field.value.kind == "OpenJson" then
          [ "deserialize_with = ${quote (helperName record field)}" ]
        else
          [ ]
      );
      omit =
        if field.rustEncode == "OmitAbsent" then
          [ ''skip_serializing_if = "Option::is_none"'' ]
        else if field.rustEncode == "OmitEmpty" then
          [
            "skip_serializing_if = ${
              quote (
                if field.value.kind == "Map" then
                  "BTreeMap::is_empty"
                else if field.value.unique then
                  "UniqueVec::is_empty"
                else if record.rust.emission == "Borrowed" then
                  "<[${owned field.value.element}]>::is_empty"
                else
                  "Vec::is_empty"
              )
            }"
          ]
        else
          [ ];
      # Only canonical lowerCamelCase fields use the container convention.
      rename = lib.optional (
        !(
          builtins.match "[a-z][a-zA-Z0-9]*" field.name != null && field.rust.name == checked.snake field.name
        )
      ) "rename = ${quote field.name}";
      attrs = rename ++ decode ++ omit;
    in
    lib.optionalString (attrs != [ ]) "#[serde(${lib.concatStringsSep ", " attrs})]\n";
  renderRecord =
    record:
    if record.rust.emission == "MemberNamesOnly" then
      "${visibility record.rust.visibility}const ${record.rust.name}: &str = ${quote (builtins.head record.fields).name};\n"
    else
      ''
        #[derive(${
          lib.concatStringsSep ", " (
            record.rust.derives
            ++ [ "serde::Serialize" ]
            ++ lib.optional (record.decoder != "NoDecoder") "serde::Deserialize"
          )
        })]
        #[serde(rename_all = "camelCase"${
          lib.optionalString (record.decoder == "RejectUnknown") ", deny_unknown_fields"
        })]
        ${visibility record.rust.visibility}struct ${record.rust.name}${
          lib.optionalString (record.rust.emission == "Borrowed") "<'a>"
        } {
        ${lib.concatMapStrings (field: ''
          ${attributes record field}${visibility field.rust.visibility}${field.rust.name}: ${fieldType record field},
        '') record.fields}
        }
        ${lib.concatMapStrings (helper record) record.fields}
      '';
  renderVocabulary =
    vocabulary:
    if lib.hasPrefix "status " vocabulary.coordinate then
      ''
        db_status! {
            ${vocabulary.rust.name} {
            ${lib.concatMapStringsSep "\n" (
              member: "${checked.variant member} => ${quote member},"
            ) vocabulary.members}
            }
        }
      ''
    else
      ''
        #[derive(${
          lib.concatStringsSep ", " (
            vocabulary.rust.derives
            ++ [ "serde::Serialize" ]
            ++ lib.optional (vocabulary.decoder == "Closed") "serde::Deserialize"
          )
        })]
        ${visibility vocabulary.rust.visibility}enum ${vocabulary.rust.name} {
        ${lib.concatMapStrings (member: ''
          #[serde(rename = ${quote member})]
          ${checked.variant member},
        '') vocabulary.members}
        }
      '';
  fragments =
    map (record: {
      file = record.rust.file;
      body = renderRecord record;
    }) checked.records
    ++ map (vocabulary: {
      file = vocabulary.rust.file;
      body = renderVocabulary vocabulary;
    }) checked.vocabularies;
in
builtins.mapAttrs (
  _: entries:
  "// @generated by nix/meta/rust.nix; regenerate with nix run .#regenerate.\n\n"
  + lib.concatMapStringsSep "\n" (entry: entry.body) entries
) (lib.groupBy (entry: entry.file) fragments)
