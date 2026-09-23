# Presentation only: authored sections, related topics and checked metadata selections.
# Existing declaration-to-topic references supply membership without repetition.
let
  fragment = file: heading: { inherit file heading; };
  entry = kind: id: { inherit kind id; };
  category = kind: { inherit kind; };
  option = path: {
    kind = "option";
    inherit path;
  };
  namespace = path: {
    kind = "option-namespace";
    inherit path;
  };
in
{
  commands = {
    select = [ ];
    fragments = [
      (fragment ../../docs/CONTRACT.md "## Native command parsing")
    ];
    related = [
      "outputs"
      "errors"
    ];
  };
  runtime = {
    select = [
      (entry "command" "check")
      (entry "command" "run")
      (entry "command" "ps")
      (entry "command" "down")
      (entry "command" "clean")
      (entry "package" "root/nixfied-runtime")
      (entry "record" "primitive/Manifest")
      (entry "record" "local/CheckOutput")
      (entry "record" "local/DownReport")
      (entry "record" "local/CleanupOutcome")
      (entry "record" "output-schema/ps-json")
    ];
    fragments = [
      (fragment ../../docs/ARCHITECTURE.md "## The load-bearing decision: two tools, one seam")
      (fragment ../../docs/ARCHITECTURE.md "## Correctness in four layers")
      (fragment ../../docs/ARCHITECTURE.md "## Identity & placement")
      (fragment ../../docs/ARCHITECTURE.md "## Registry, liveness, leases")
      (fragment ../../docs/ARCHITECTURE.md "## Ports, state, containment")
    ];
    related = [
      "manifest"
      "services"
      "state"
      "outputs"
    ];
  };
  authoring = {
    select = [
      (entry "function" "library/compileManifest")
      (entry "function" "library/projectApps")
      (entry "function" "library/seq")
      (category "argument")
      (namespace [
        "nixfied"
        "project"
      ])
      (option [
        "nixfied"
        "tasks"
      ])
      (option [
        "nixfied"
        "services"
      ])
      (option [
        "nixfied"
        "surface"
        "verbs"
      ])
    ];
    fragments = [
      (fragment ../../docs/GUIDE.md "## Author `nixfied.nix`")
    ];
    related = [
      "tasks"
      "services"
      "context"
    ];
  };
  tasks = {
    select = [
      (entry "function" "library/seq")
      (entry "command" "run")
      (entry "record" "primitive/TaskSpec")
      (entry "record" "primitive/StepSpec")
      (entry "record" "primitive/Invocation")
      (entry "record" "primitive/ExitPolicy")
      (option [
        "nixfied"
        "tasks"
        "<name>"
        "kind"
      ])
      (option [
        "nixfied"
        "tasks"
        "<name>"
        "invocation"
      ])
      (option [
        "nixfied"
        "tasks"
        "<name>"
        "requires"
      ])
      (option [
        "nixfied"
        "tasks"
        "<name>"
        "serviceLifetime"
      ])
      (option [
        "nixfied"
        "tasks"
        "<name>"
        "steps"
      ])
      (option [
        "nixfied"
        "tasks"
        "<name>"
        "exitPolicy"
      ])
      (option [
        "nixfied"
        "tasks"
        "<name>"
        "defaultOutput"
      ])
    ];
    fragments = [
      (fragment ../../docs/GUIDE.md "## Author `nixfied.nix`")
      (fragment ../../docs/GUIDE.md "## Run and control")
    ];
    related = [
      "services"
      "outputs"
      "context"
    ];
  };
  services = {
    select = [
      (entry "record" "primitive/ServiceSpec")
      (entry "record" "primitive/Lifecycle")
      (entry "record" "primitive/ProbeSpec")
      (entry "record" "primitive/Endpoint")
      (option [
        "nixfied"
        "services"
        "<name>"
        "connectsTo"
      ])
      (option [
        "nixfied"
        "services"
        "<name>"
        "containment"
      ])
      (option [
        "nixfied"
        "services"
        "<name>"
        "endpoints"
      ])
      (option [
        "nixfied"
        "services"
        "<name>"
        "primaryEndpoint"
      ])
      (option [
        "nixfied"
        "services"
        "<name>"
        "lifecycle"
        "prepare"
      ])
      (option [
        "nixfied"
        "services"
        "<name>"
        "lifecycle"
        "start"
      ])
      (option [
        "nixfied"
        "services"
        "<name>"
        "lifecycle"
        "ready"
      ])
      (option [
        "nixfied"
        "services"
        "<name>"
        "lifecycle"
        "health"
      ])
      (option [
        "nixfied"
        "services"
        "<name>"
        "lifecycle"
        "stop"
      ])
      (option [
        "nixfied"
        "services"
        "<name>"
        "lifecycle"
        "clean"
      ])
    ];
    fragments = [
      (fragment ../../docs/ADAPTERS.md "## What an adapter provides")
      (fragment ../../docs/ADAPTERS.md "## Conventions")
      (fragment ../../docs/ADAPTERS.md "## Multiple listeners: declare every endpoint")
      (fragment ../../docs/ADAPTERS.md "## Endpoint-less services: durable is not listening")
    ];
    related = [
      "state"
      "placeholders"
      "adapters"
    ];
  };
  state = {
    select = [
      (namespace [
        "nixfied"
        "state"
      ])
      (namespace [
        "nixfied"
        "slotPolicy"
      ])
      (namespace [
        "nixfied"
        "placement"
      ])
      (option [
        "nixfied"
        "services"
        "<name>"
        "stateRefs"
      ])
      (option [
        "nixfied"
        "tasks"
        "<name>"
        "serviceLifetime"
      ])
      (entry "record" "primitive/StatePolicy")
      (entry "record" "primitive/SlotPolicy")
      (entry "record" "primitive/Placement")
      (entry "command" "ps")
      (entry "command" "down")
      (entry "command" "clean")
    ];
    fragments = [
      (fragment ../../docs/GUIDE.md "## Services, slots, and state")
    ];
    related = [
      "context"
      "secrets"
      "recovery"
    ];
  };
  placeholders = {
    select = [
      (entry "record" "primitive/Endpoint")
      (entry "record" "primitive/Invocation")
      (entry "record" "primitive/SecretDescriptor")
      (option [
        "nixfied"
        "services"
        "<name>"
        "endpoints"
      ])
      (option [
        "nixfied"
        "services"
        "<name>"
        "primaryEndpoint"
      ])
      (option [
        "nixfied"
        "services"
        "<name>"
        "connectsTo"
      ])
      (option [
        "nixfied"
        "services"
        "<name>"
        "lifecycle"
        "start"
        "invocation"
        "env"
      ])
      (option [
        "nixfied"
        "tasks"
        "<name>"
        "requires"
      ])
      (option [
        "nixfied"
        "tasks"
        "<name>"
        "invocation"
        "env"
      ])
    ];
    fragments = [
      (fragment ../../docs/ADAPTERS.md "## Endpoints and placeholders")
    ];
    related = [
      "services"
      "secrets"
    ];
  };
  secrets = {
    select = [
      (namespace [
        "nixfied"
        "secrets"
      ])
      (entry "record" "primitive/SecretDescriptor")
      (entry "record" "primitive/SecretSource")
      (option [
        "nixfied"
        "tasks"
        "<name>"
        "invocation"
        "env"
      ])
      (option [
        "nixfied"
        "services"
        "<name>"
        "lifecycle"
        "start"
        "invocation"
        "env"
      ])
    ];
    fragments = [
      (fragment ../../docs/GUIDE.md "## Secrets")
    ];
    related = [
      "context"
      "outputs"
    ];
  };
  adapters = {
    select = [
      (category "module")
      (entry "argument" "module-argument/adapters")
      (option [
        "nixfied"
        "services"
        "<name>"
        "lifecycle"
      ])
      (namespace [
        "nixfied"
        "placement"
      ])
    ];
    fragments = [
      (fragment ../../docs/ADAPTERS.md "## Import and compose adapters")
      (fragment ../../docs/ADAPTERS.md "## What an adapter provides")
      (fragment ../../docs/ADAPTERS.md "## Parameterization")
    ];
    related = [
      "services"
      "authoring"
    ];
  };
  context = {
    select = [
      (category "argument")
      (namespace [
        "nixfied"
        "codebases"
        "main"
      ])
      (option [
        "nixfied"
        "target"
        "system"
      ])
      (option [
        "nixfied"
        "tasks"
        "<name>"
        "invocation"
        "codebaseId"
      ])
      (option [
        "nixfied"
        "tasks"
        "<name>"
        "invocation"
        "cwd"
      ])
      (option [
        "nixfied"
        "tasks"
        "<name>"
        "invocation"
        "env"
      ])
      (option [
        "nixfied"
        "tasks"
        "<name>"
        "invocation"
        "tools"
      ])
      (entry "record" "primitive/Codebase")
      (entry "record" "primitive/SourcePolicy")
      (entry "record" "primitive/Invocation")
    ];
    fragments = [
      (fragment ../../docs/GUIDE.md "## Source and invocation context")
    ];
    related = [
      "secrets"
      "state"
    ];
  };
  outputs = {
    select = [
      (entry "command" "run")
      (option [
        "nixfied"
        "tasks"
        "<name>"
        "defaultOutput"
      ])
      (entry "record" "output-schema/run-json")
      (entry "record" "output-schema/run-summary-json")
      (entry "record" "output-schema/run-task")
      (entry "record" "output-schema/run-node")
      (entry "record" "output-schema/run-service")
      (entry "record" "output-schema/ps-json")
      (entry "record" "output-schema/runtime-error-projection")
    ];
    fragments = [
      (fragment ../../docs/GUIDE.md "### Choose output")
      (fragment ../../docs/CONTRACT.md "## Output and failure contract")
    ];
    related = [
      "errors"
      "commands"
    ];
  };
  errors = {
    select = [
      (category "error")
      (entry "record" "output-schema/runtime-error")
      (entry "record" "output-schema/runtime-error-cause")
      (entry "record" "output-schema/runtime-error-projection")
      (entry "record" "output-schema/port-conflict")
      (entry "record" "local/RegistryIdentityDiagnostic")
      (entry "record" "output-schema/run-task")
      (entry "record" "output-schema/runtime-error-port-conflict")
      (entry "record" "output-schema/port-conflict-endpoint")
      (entry "record" "output-schema/nixfied-owner")
    ];
    fragments = [
      (fragment ../../docs/CONTRACT.md "## Runtime error diagnostics")
    ];
    related = [
      "recovery"
      "outputs"
    ];
  };
  recovery = {
    select = [
      (entry "command" "ps")
      (entry "command" "down")
      (entry "command" "clean")
      (entry "package" "root/upgrade")
    ];
    fragments = [
      (fragment ../../docs/GUIDE.md "## Upgrade and recover")
    ];
    related = [
      "state"
      "manifest"
    ];
  };
  discovery = {
    select = [
      (entry "app" "project/run")
      (entry "app" "project/manifest-check")
      (entry "app" "project/ps")
      (entry "app" "project/down")
      (entry "app" "project/clean")
      (option [
        "nixfied"
        "surface"
        "verbs"
      ])
      (entry "package" "root/docs")
    ];
    fragments = [
      (fragment ../../docs/GUIDE.md "## Discover the project surface")
    ];
    related = [
      "authoring"
      "commands"
    ];
  };
  manifest = {
    select = [
      (entry "command" "check")
      (entry "record" "primitive/Manifest")
      (entry "record" "primitive/Generator")
      (entry "record" "primitive/Target")
      (entry "record" "primitive/ClosureSpec")
    ];
    fragments = [
      (fragment ../../docs/CONTRACT.md "## Manifest and version boundary")
    ];
    related = [
      "runtime"
      "derivation"
    ];
  };
  derivation = {
    select = [
      (entry "record" "primitive/TaskSpec")
      (entry "record" "primitive/StepSpec")
      (entry "record" "primitive/ClosureSpec")
      (entry "record" "primitive/TerminalSemantics")
      (option [
        "nixfied"
        "tasks"
        "<name>"
        "steps"
      ])
      (option [
        "nixfied"
        "tasks"
        "<name>"
        "requires"
      ])
      (option [
        "nixfied"
        "closures"
        "<name>"
        "operationBindings"
      ])
    ];
    fragments = [
      (fragment ../../docs/DERIVATION_SPEC.md "## Specification ownership and scope")
      (fragment ../../docs/DERIVATION_SPEC.md "## 1. Identifiers and canonical order")
      (fragment ../../docs/DERIVATION_SPEC.md "## 2. Flattening and step paths")
      (fragment ../../docs/DERIVATION_SPEC.md "## 3. `servicesRequired(task)`")
      (fragment ../../docs/DERIVATION_SPEC.md "## 4. `operationBindings(closure)`")
      (fragment ../../docs/DERIVATION_SPEC.md "## 5. Default operation ids and terminal tokens")
      (fragment ../../docs/DERIVATION_SPEC.md "## 6. Golden vectors")
    ];
    related = [
      "manifest"
      "tasks"
    ];
  };
  development = {
    select = [
      (entry "package" "check/rust-workspace")
      (entry "package" "check/derive-facts-vectors")
      (entry "package" "check/nixfied-runtime")
      (entry "package" "check/minimal-manifest")
      (entry "package" "devShell/default")
    ];
    fragments = [
      (fragment ../../docs/DEVELOPMENT.md "## Canonical local checks")
      (fragment ../../docs/DEVELOPMENT.md "## Verify uncommitted downstream changes")
      (fragment ../../docs/DEVELOPMENT.md "## Change-specific verification")
    ];
    related = [
      "derivation"
      "manifest"
    ];
  };
}
