# Checked publication metadata; native bindings remain lazy until selected.
{ lib }:
{
  declarations,
  targets ? [ ],
}:
let
  nonBlank = value: builtins.isString value && builtins.match "[[:space:]]*" value == null;
  forms = {
    function = {
      scopes = [ "library" ];
      fields = [
        "input"
        "result"
      ];
    };
    module = {
      scopes = [ "adapter" ];
      fields = [ "contributes" ];
    };
    argument = {
      scopes = [ "module-argument" ];
      fields = [
        "valueDomain"
        "provider"
      ];
    };
    app = {
      scopes = [
        "root"
        "project"
      ];
      fields = [ "effects" ];
    };
    package = {
      scopes = [
        "root"
        "check"
        "devShell"
      ];
      fields = [ "artifact" ];
    };
  };
  normalize =
    declaration:
    let
      form = forms.${declaration.kind or ""} or (throw "Nixfied publication: unsupported kind");
      required = [
        "kind"
        "scope"
        "name"
        "description"
        "usage"
      ]
      ++ form.fields;
      allowed =
        required
        ++ [
          "binding"
          "references"
        ]
        ++ lib.optionals (declaration.kind == "app") [
          "topic"
          "command"
        ];
    in
    assert lib.assertMsg (
      builtins.removeAttrs declaration allowed == { }
    ) "Nixfied publication: unknown or wrong-kind fields";
    assert lib.assertMsg (builtins.all (
      field: nonBlank (declaration.${field} or null)
    ) required) "Nixfied publication: missing or blank metadata";
    assert lib.assertMsg (builtins.elem declaration.scope form.scopes)
      "Nixfied publication: wrong scope for kind";
    assert lib.assertMsg (
      builtins.match "[A-Za-z0-9][A-Za-z0-9_-]*" declaration.name != null
    ) "Nixfied publication: invalid name";
    assert lib.assertMsg (declaration ? binding) "Nixfied publication: missing native binding";
    assert lib.assertMsg (
      declaration.kind != "app" || (declaration ? topic) != (declaration ? command)
    ) "Nixfied publication: app requires exactly one native topic or command reference";
    (builtins.removeAttrs declaration [
      "binding"
      "references"
    ])
    // {
      id = "${declaration.scope}/${declaration.name}";
      references =
        (declaration.references or [ ])
        ++ lib.optional (declaration ? topic) {
          kind = "topic";
          id = declaration.topic;
        }
        ++ lib.optional (declaration ? command) {
          kind = "command";
          id = declaration.command;
        };
    };
  entries = map normalize declarations;
  identities = map (entry: { inherit (entry) kind id; }) entries;
  key = identity: builtins.toJSON identity;
  keys = map key identities;
  available = map key (identities ++ targets);
  validReference =
    reference:
    assert lib.assertMsg (
      builtins.isAttrs reference
      && nonBlank (reference.kind or null)
      && (
        if reference.kind == "option" then
          builtins.attrNames reference == [
            "kind"
            "path"
          ]
          && builtins.isList reference.path
          && reference.path != [ ]
          && builtins.all nonBlank reference.path
        else
          builtins.attrNames reference == [
            "id"
            "kind"
          ]
          && nonBlank reference.id
      )
    ) "Nixfied publication: malformed reference";
    assert lib.assertMsg (builtins.elem (key reference) available)
      "Nixfied publication: missing or wrong-kind reference ${builtins.toJSON reference}";
    true;
  valid =
    assert lib.assertMsg (lib.unique keys == keys) "Nixfied publication: duplicate identity";
    builtins.deepSeq entries (
      builtins.all (
        entry:
        assert lib.assertMsg (builtins.isList entry.references)
          "Nixfied publication: references must be a list";
        builtins.all validReference entry.references
      ) entries
    );
  namespaces = builtins.mapAttrs (
    _: scoped:
    builtins.mapAttrs (
      _: declarations:
      builtins.listToAttrs (
        map (d: {
          name = d.name;
          value = d;
        }) declarations
      )
    ) (lib.groupBy (d: d.scope) scoped)
  ) (lib.groupBy (d: d.kind) declarations);
  select = kind: scope: namespaces.${kind}.${scope} or { };
  bound =
    declaration:
    let
      value = declaration.binding;
      kind = declaration.kind;
      validValue =
        if kind == "function" then
          lib.isFunction value
        else if kind == "module" then
          lib.isFunction value || builtins.isAttrs value || builtins.isPath value
        else if kind == "package" then
          lib.isDerivation value
        else if kind == "app" then
          builtins.isAttrs value && (value.type or null) == "app" && builtins.isString (value.program or null)
        else
          true; # Argument domains are enforced by their native consumers.
    in
    assert lib.assertMsg validValue
      "Nixfied publication ${declaration.name}: wrong native binding kind";
    if kind == "app" then
      value
      // {
        meta = (value.meta or { }) // {
          inherit (declaration) description;
        };
      }
    else
      value;
in
builtins.seq valid {
  inherit entries;
  project = kind: scope: builtins.mapAttrs (_: bound) (select kind scope);
  names = kind: scope: builtins.attrNames (select kind scope);
  audit =
    kind: scope: actual:
    assert lib.assertMsg (
      builtins.attrNames actual == builtins.attrNames (select kind scope)
    ) "Nixfied publication: final exports differ from descriptors (${kind}/${scope})";
    true;
}
