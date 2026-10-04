# Representative authored selections and rendered links; graph and extraction
# algorithms have independent vectors in docs-navigation.nix and reference.nix.
{ lib, index }:
let
  has = lib.hasInfix;
  before =
    first: second: text:
    has first text && has second (lib.last (lib.splitString first text));
  entry =
    kind: id:
    lib.findFirst (x: x.kind == kind && x.id == id) (throw "missing reference ${kind}/${id}") (
      index.api ++ index.options
    );
  cases = {
    adapters = {
      kind = "module";
      id = "adapter/postgres";
    };
    development = {
      kind = "app";
      id = "root/regenerate";
    };
    errors = {
      kind = "error";
      id = "SECRET_UNAVAILABLE";
    };
  };
  outputs = index.topics.outputs;
  error = entry "error" "SECRET_UNAVAILABLE";
  errorRecord = entry "record" "output-schema/runtime-error";
in
assert lib.all (
  name:
  let
    topic = index.topics.${name};
    required = cases.${name};
    excluded = if name == "development" then "### command run" else "### app root/regenerate";
    requiredText =
      if required.kind == "error" then
        "docs api error ${required.id}"
      else
        "### ${required.kind} ${required.id}";
  in
  builtins.any (x: x.kind == required.kind && x.id == required.id) topic.members
  && has requiredText topic.text
  && !(has excluded topic.text)
  && lib.hasPrefix "Topic: ${name}\n\n${topic.prose}" topic.text
) (builtins.attrNames cases);
assert has "Details: `docs api error SECRET_UNAVAILABLE`" index.topics.errors.text;
assert has "Related topic: `docs topic ${error.contextTopic}`" index.topics.errors.text;
assert !(has "### error SECRET_UNAVAILABLE" index.topics.errors.text);
assert has "docs api error <code>" errorRecord.text;
assert has "docs topic context" (entry "option" "nixfied.target.system").text;
assert !(has "docs topic state" (entry "option" "nixfied.target.system").text);
assert has "docs topic state" (entry "option" "nixfied.services.<name>.stateRefs").text;
assert !(has "docs topic state`" index.topics.state.text);
assert
  lib.sort builtins.lessThan (lib.unique (map (x: x.document) outputs.fragments)) == [
    "CONTRACT.md"
    "GUIDE.md"
  ];
assert (builtins.head outputs.fragments).document == "GUIDE.md";
assert (lib.last outputs.fragments).document == "CONTRACT.md";
assert before "### Choose output" "## Output and failure contract" outputs.prose;
true
