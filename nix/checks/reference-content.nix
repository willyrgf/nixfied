# Literal topic and navigation expectations, independent of the renderer.
{ lib, index }:
let
  has = lib.hasInfix;
  words =
    text:
    lib.concatStringsSep " " (
      builtins.filter (x: builtins.isString x && x != "") (builtins.split "[[:space:]]+" text)
    );
  before =
    first: second: text:
    has first text && has second (lib.last (lib.splitString first text));
  entry =
    kind: id:
    lib.findFirst (x: x.kind == kind && x.id == id) (throw "missing reference ${kind}/${id}") (
      index.api ++ index.options
    );
  ref = kind: id: { inherit kind id; };
  linked =
    kind: id: direction: target:
    builtins.elem target (entry kind id).${direction};
  cases = {
    adapters = ref "module" "adapter/postgres";
    authoring = ref "argument" "module-argument/pkgs";
    commands = ref "command" "install";
    context = ref "option" "nixfied.codebases.main.sourceMode";
    derivation = ref "function" "library/seq";
    development = ref "app" "root/regenerate";
    discovery = ref "app" "project/docs";
    errors = ref "error" "SECRET_UNAVAILABLE";
    manifest = ref "function" "library/compileManifest";
    outputs = ref "record" "output-schema/run-summary-json";
    placeholders = ref "error" "PORT_CONFLICT";
    recovery = ref "command" "upgrade";
    runtime = ref "command" "run";
    secrets = ref "option" "nixfied.secrets.<name>.source.kind";
    services = ref "record" "primitive/Lifecycle";
    state = ref "option" "nixfied.state.persistence";
    tasks = ref "option" "nixfied.tasks.<name>.requires";
  };
  journeys = {
    state = [
      "Run `down` first."
      "Purge overrides retention only"
      "never the ownership, confinement, or live-process checks"
      "runtime-owned slot root"
      "Child-tool caches remain project-owned."
    ];
    runtime = [
      "projectId / environment / slot / runId"
      "Service attribution belongs to process evidence"
      "Rust materialises *host-absolute* placement at admission"
    ];
    adapters = [
      "An adapter is a Nix module"
      "imports = [ adapters.postgres ];"
      "the adopter references them as steps in its own composites"
      "contributes *definitions only*"
    ];
    derivation = [
      "Status: **normative**."
      "no equality check of"
      "flatten(ci) ="
      "ci.check.fmt"
      ''servicesRequired(all) = ["api", "postgres", "worker"]''
    ];
    tasks = [ "Invocations observe the live workspace by default" ];
    development = [
      "nix run .#gate -- --dirty"
      "does not consume other uncommitted framework changes"
      "Use the smallest proof that covers the change"
      "Contract or cross-layer change"
    ];
    outputs = [
      "Requesting `task-output` for a composite is rejected"
      "For an interactive run, use `summary`"
      "--output json"
      "--output task-output"
    ];
  };
  derivation = index.topics.derivation.prose;
  outputs = index.topics.outputs;
  errorRecord = entry "record" "output-schema/runtime-error";
  errorBacklinks = map (x: x.id) (builtins.filter (x: x.kind == "error") errorRecord.backlinks);
  renderedBacklinks = lib.last (lib.splitString "### Referenced by" errorRecord.text);
in
assert builtins.attrNames cases == builtins.attrNames index.topics;
assert lib.all (
  name:
  let
    topic = index.topics.${name};
    required = cases.${name};
    excluded = if name == "development" then ref "command" "run" else ref "app" "root/regenerate";
    members = map (x: ref x.kind x.id) topic.members;
    requiredText =
      if required.kind == "error" then
        "docs api error ${required.id}"
      else
        "### ${required.kind} ${required.id}";
  in
  builtins.elem required members
  && !(builtins.elem excluded members)
  && has requiredText topic.text
  && !(has "### ${excluded.kind} ${excluded.id}" topic.text)
  && lib.hasPrefix "Topic: ${name}\n\n${topic.prose}" topic.text
) (builtins.attrNames cases);
assert lib.all (
  name: lib.all (text: has text (words index.topics.${name}.prose)) journeys.${name}
) (builtins.attrNames journeys);
assert before "## 5. Golden vectors" "### 5.1 Representative examples" derivation;
assert
  lib.sort builtins.lessThan (
    lib.unique (
      map (line: builtins.head (builtins.match "#### V([0-9]+) .*" line)) (
        builtins.filter (line: builtins.match "#### V([0-9]+) .*" line != null) (
          lib.splitString "\n" derivation
        )
      )
    )
  ) == [
    "1"
    "10"
    "2"
    "3"
    "4"
    "6"
    "8"
    "9"
  ];
assert lib.all (
  name:
  let
    topic = index.topics.${name};
    errors = builtins.filter (x: x.kind == "error") topic.members;
  in
  lib.all (
    related:
    related.kind == "topic"
    && related.id != name
    && builtins.hasAttr related.id index.topics
    && has "docs topic ${related.id}" topic.text
  ) topic.related
  && (
    topic.related == [ ]
    || topic.members == [ ]
    || before "### Related topics" "## Related definitions" topic.text
  )
  && (
    errors == [ ]
    || (has "### Error codes and related topics" topic.text && !(has "### error " topic.text))
  )
  && lib.all (
    error:
    let
      definition = entry "error" error.id;
      blocks = lib.splitString "- `${error.id}`\n" topic.text;
      body = builtins.head (lib.splitString "\n- `" (lib.last blocks));
      destination =
        if definition.contextTopic == name then "This topic" else "`docs topic ${definition.contextTopic}`";
    in
    builtins.length blocks == 2
    && has definition.description body
    && has "Details: `docs api error ${error.id}`" body
    && has "Related topic: ${destination}" body
    && !(has "docs topic ${name}`" body)
  ) errors
) (builtins.attrNames index.topics);
assert before "### option nixfied.state.persistence" "### option nixfied.placement.ports.base"
  index.topics.state.text;
assert before "### record output-schema/runtime-error\n" "### record output-schema/run-task\n"
  index.topics.errors.text;
assert linked "app" "project/run" "references" (ref "command" "run");
assert linked "command" "run" "backlinks" (ref "app" "project/run");
assert linked "command" "run" "references" (ref "record" "output-schema/run-json");
assert linked "record" "output-schema/run-json" "backlinks" (ref "command" "run");
assert linked "record" "primitive/Manifest" "references" (ref "record" "primitive/TaskSpec");
assert linked "record" "primitive/TaskSpec" "backlinks" (ref "record" "primitive/Manifest");
assert linked "option" "nixfied.target.system" "references" (ref "topic" "context");
assert !(has "docs topic state" (entry "option" "nixfied.target.system").text);
assert !(has "docs topic placeholders" (entry "option" "nixfied.target.system").text);
assert !(has "docs topic derivation" errorRecord.text);
assert lib.all
  (
    pair:
    let
      definition = entry "option" pair.option;
    in
    !(has "docs topic ${pair.topic}" definition.description)
    && has "docs topic ${pair.topic}" definition.text
    && !(has "docs topic ${pair.topic}`" index.topics.${pair.topic}.text)
  )
  [
    {
      option = "nixfied.services.<name>.stateRefs";
      topic = "state";
    }
    {
      option = "nixfied.tasks.<name>.requires";
      topic = "placeholders";
    }
  ];
assert
  lib.sort builtins.lessThan (lib.unique (map (x: x.document) outputs.fragments)) == [
    "CONTRACT.md"
    "GUIDE.md"
  ];
assert (builtins.head outputs.fragments).document == "GUIDE.md";
assert (lib.last outputs.fragments).document == "CONTRACT.md";
assert before "### Choose output" "## Output and failure contract" outputs.prose;
assert has "### 5.1" derivation;
assert
  lib.sort builtins.lessThan (
    map (line: builtins.head (builtins.match "- `([^`]+)`" line)) (
      builtins.filter (line: builtins.match "- `([^`]+)`" line != null) (
        lib.splitString "\n" renderedBacklinks
      )
    )
  ) == lib.sort builtins.lessThan errorBacklinks;
assert builtins.length (lib.splitString "docs api error <code>" renderedBacklinks) == 2;
true
