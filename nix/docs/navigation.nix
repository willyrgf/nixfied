# Private presentation relationships. No bindings, runtime rules or traversal of
# the project graph enter this boundary. Topic selectors add navigation only.
{ lib }:
{ entries, topics }:
let
  nonBlank = value: builtins.isString value && builtins.match "[[:space:]]*" value == null;
  require =
    condition: message: value:
    if condition then value else throw "Nixfied docs navigation: ${message}";
  exact =
    fields: value:
    builtins.isAttrs value && builtins.attrNames value == builtins.sort builtins.lessThan fields;
  identity = entry: { inherit (entry) kind id; };
  key =
    entry:
    builtins.toJSON [
      entry.kind
      entry.id
    ];
  validIdentity = value: exact [ "kind" "id" ] value && nonBlank value.kind && nonBlank value.id;
  validPath = path: builtins.isList path && path != [ ] && lib.all nonBlank path;
  topicEntries = lib.mapAttrsToList (id: topic: {
    kind = "topic";
    inherit id;
    references = map (related: {
      kind = "topic";
      id = related;
    }) topic.related;
  }) topics;
  allEntries = entries ++ topicEntries;
  keys = map key allEntries;
  targets = builtins.listToAttrs (
    map (entry: {
      name = key entry;
      value = identity entry;
    }) allEntries
  );
  resolve =
    ref:
    require (validIdentity ref) "malformed reference" (
      require (builtins.hasAttr (key ref) targets) "unknown target ${key ref}" ref
    );
  select =
    selector:
    let
      kind = if builtins.isAttrs selector then selector.kind or null else null;
      matched =
        if
          builtins.elem kind [
            "option"
            "option-namespace"
          ]
        then
          require (exact [ "kind" "path" ] selector && validPath selector.path)
            "option selectors require a nonempty native path"
            (
              builtins.filter (
                entry:
                entry.kind == "option"
                && (
                  if kind == "option" then
                    entry.loc == selector.path
                  else
                    lib.take (builtins.length selector.path) entry.loc == selector.path
                )
              ) entries
            )
        else if exact [ "kind" "id" ] selector && nonBlank selector.id then
          builtins.filter (entry: entry.kind == kind && entry.id == selector.id) entries
        else if exact [ "kind" ] selector && nonBlank kind && kind != "vocabulary" then
          builtins.filter (entry: entry.kind == kind) entries
        else
          throw "Nixfied docs navigation: malformed topic selector";
    in
    require (matched != [ ]) "empty topic selection ${builtins.toJSON selector}" (map identity matched);
  selected = builtins.mapAttrs (_: topic: lib.unique (lib.concatMap select topic.select)) topics;
  references =
    entry:
    lib.unique (
      map resolve entry.references
      ++ lib.concatMap (
        topic:
        lib.optional (builtins.elem (identity entry) selected.${topic}) {
          kind = "topic";
          id = topic;
        }
      ) (builtins.attrNames topics)
    );
  forward = builtins.listToAttrs (
    map (entry: {
      name = key entry;
      value = references entry;
    }) allEntries
  );
  backward = builtins.listToAttrs (
    map (entry: {
      name = key entry;
      value = map identity (
        builtins.filter (other: builtins.elem (identity entry) forward.${key other}) allEntries
      );
    }) allEntries
  );
  checked =
    require (lib.all (entry: validIdentity (identity entry)) allEntries) "invalid entry identity"
      (
        require (lib.unique keys == keys) "duplicate entry identity" (
          builtins.deepSeq selected (builtins.deepSeq forward true)
        )
      );
in
builtins.seq checked {
  inherit key;
  references = entry: forward.${key entry};
  backlinks = entry: backward.${key entry};
  # Selectors express a reading order; incoming declarations add discoverable
  # members without overriding that order or duplicating their ownership.
  members =
    name:
    lib.unique (
      selected.${name}
      ++
        builtins.filter (entry: entry.kind != "topic")
          backward.${
            key {
              kind = "topic";
              id = name;
            }
          }
    );
}
