# Syntax facts only. Entry routing, acquisition and repetition remain native.
{
  lib,
  structure ? import ./default.nix { inherit lib; },
}:
let
  presentation = import ./command-help.nix { inherit lib; };
  absent = {
    kind = "Absent";
  };
  literal = value: {
    kind = "Literal";
    inherit value;
  };
  argument = id: token: valueDomain: initialValue: help: {
    binding = "Command";
    inherit
      id
      token
      valueDomain
      initialValue
      help
      ;
  };
  domain = kind: { inherit kind; };
  visible = text: metavar: {
    kind = "Visible";
    inherit text metavar;
  };
  hidden = explanation: {
    kind = "Hidden";
    inherit explanation;
  };
  shared = arg: arg // { binding = "Shared"; };
  model = shared (
    argument "model" "--model" (domain "Path") absent (
      hidden "Framework-supplied compiled model path; native admission requires a model and checks its origin."
    )
  );
  allow = shared (
    argument "allowNonStoreModel" "--allow-non-store-model" (domain "Flag") (literal false) (
      hidden "Test-only origin-policy escape hatch; generated apps do not supply it."
    )
  );
  state = shared (
    argument "stateBase" "--state-base" (domain "Path") absent (
      hidden "Explicit host state base; native environment precedence applies when absent."
    )
  );
  slot = shared (
    argument "slot" "--slot" {
      kind = "Unsigned";
      bits = 32;
    } absent (visible "Select a declared project slot" "<number>")
  );
  timeout =
    text:
    argument "timeoutMs" "--timeout-ms" {
      kind = "Unsigned";
      bits = 64;
    } (literal 5000) (visible text "<number>");
  ref = kind: id: { inherit kind id; };
  runtime = name: description: arguments: column: output: {
    inherit name description arguments;
    references = [
      (ref "topic" "commands")
      (ref "record" output)
      (ref "record" "output-schema/runtime-error")
    ];
    renderHelp =
      facts:
      lib.concatStringsSep "\n" (
        [
          facts.description
          ""
          "Usage:"
          (
            "  nix run .#${facts.apps.${name}.name} -- "
            + lib.optionalString (name == "run") "${presentation.label facts.args.task} "
            + "[options]"
          )
        ]
        ++ lib.optional (name == "run") "  nix run .#<verb> -- [options]"
        ++ [
          ""
          "Options:"
          (presentation.runtimeRows column facts)
        ]
      );
  };
  compact = name: description: arguments: topic: {
    inherit name description arguments;
    references = [
      (ref "topic" topic)
      (ref "topic" "commands")
    ];
    renderHelp = facts: "usage: nixfied ${facts.name}${presentation.optionalArguments facts.arguments}";
  };
  modes = structure.vocabularyMap."run-output-mode".members;
  mode =
    variant:
    lib.findFirst (
      member: structure.variant member == variant
    ) (throw "Missing native run-output-mode variant ${variant}") modes;
in
[
  (runtime "check" "Admit the compiled Nixfied model without executing tasks." [
    model
    allow
    slot
  ] 19 "local/CheckOutput")
  (runtime "run" "Run one declared task and its required services." [
    model
    allow
    state
    (argument "task" "--task" (domain "Text") absent (
      visible "Select a declared task (the exported verb preselects it)" "<id>"
    ))
    slot
    (timeout "Set the runtime operation timeout in milliseconds")
    (argument "output" "--output"
      {
        kind = "Enum";
        coordinate = "run-output-mode";
      }
      absent
      (
        visible "Select ${presentation.choices modes} (default: task default or ${mode "Summary"})\n${mode "TaskOutput"} requires one directly selected leaf and replays redacted output" "<mode>"
      )
    )
  ] 26 "output-schema/run-json")
  (runtime "ps" "Reconcile and report Nixfied-owned processes for a slot." [
    model
    allow
    state
    slot
  ] 19 "output-schema/ps-json")
  (runtime "down" "Stop Nixfied-owned process groups for a slot." [
    model
    allow
    state
    slot
    (timeout "Set the stop timeout in milliseconds")
  ] 25 "local/DownReport")
  (runtime "clean" "Safely clean Nixfied-owned state for a slot." [
    model
    allow
    state
    slot
    (argument "purge" "--purge" (domain "Flag") (literal false) (
      visible "Relax only the protected/persistent cleanup policy gate" null
    ))
  ] 19 "local/CleanupOutcome")
  (compact "install" "Create the native Nixfied scaffold with ownership checks." [
    (argument "root" "--root" (domain "Path") (literal ".") (
      visible "Target project directory; resolved by the native installer." "PATH"
    ))
    (argument "projectId" "--project-id" (domain "Text") absent (
      visible "Explicit project identifier; native inference and validation follow parsing." "ID"
    ))
    (argument "name" "--name" (domain "Text") absent (
      visible "Explicit display name; native inference applies when absent." "NAME"
    ))
    (argument "nixfiedUrl" "--nixfied-url" (domain "Text") (literal "github:willyrgf/nixfied") (
      visible "Input pin written into the new scaffold." "URL"
    ))
  ] "authoring")
  (compact "upgrade"
    "Repin and preflight existing Nixfied wiring while preserving project declarations."
    [
      (argument "root" "--root" (domain "Path") (literal ".") (
        visible "Existing project directory." "PATH"
      ))
      (argument "nixfiedUrl" "--nixfied-url" (domain "Text") (literal "") (
        visible "Requested input pin; empty initial value preserves the current selection." "URL"
      ))
      (argument "plan" "--plan" (domain "Flag") (literal false) (
        visible "Report the native upgrade plan without applying changes." null
      ))
      (argument "noLock" "--no-lock" (domain "Flag") (literal false) (
        visible "Skip the native lock update; the parser retains inverse update_lock state." null
      ))
    ]
    "recovery"
  )
]
