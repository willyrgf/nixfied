# Metadata is available without demanding any program or project input.
{
  pkgs ? throw "project app package set demanded",
  system ? throw "project app system demanded",
  moduleRoot ? throw "project app root demanded",
  runtimeBin ? throw "project runtime demanded",
  manifestJson ? throw "project manifest demanded",
  docs ? throw "project reference package demanded",
}:
let
  control = name: command: description: effects: {
    kind = "app";
    scope = "project";
    inherit
      name
      description
      effects
      command
      ;
    usage = "nix run .#${name} -- --help";
    binding =
      let
        syntax = (import ./meta/command-default.nix { inherit (pkgs) lib; }).byName.${command};
        program = pkgs.writeShellApplication {
          inherit name;
          text = ''exec "${runtimeBin}" ${syntax.name} ${syntax.args.manifest.token} "${manifestJson}" "$@"'';
        };
      in
      {
        type = "app";
        program = "${program}/bin/${name}";
      };
  };
in
[
  {
    kind = "app";
    scope = "project";
    name = "help";
    description = "List this flake's runnable commands";
    usage = "nix run .#help";
    effects = "Evaluates the current flake app metadata through Nix; checks that its source matches the defining project.";
    topic = "discovery";
    binding = import ./help-app.nix {
      inherit pkgs system;
      expectedFlakePath = moduleRoot;
      flakeRef = ".";
    };
  }
  {
    kind = "app";
    scope = "project";
    name = "docs";
    description = "Read the authoring and API reference from this project's Nixfied input";
    usage = "nix run .#docs -- option 'nixfied.services.<name>.stateRefs'";
    effects = "Reads packaged reference content; no manifest admission, runtime state, or network access.";
    topic = "discovery";
    binding = {
      type = "app";
      program = "${docs}/bin/nixfied-docs";
    };
  }
  (control "run" "run" "Run one declared Nixfied task"
    "Admits the manifest, prepares owned state and runs the selected task and required services."
  )
  (control "manifest-check" "check" "Admit the compiled Nixfied manifest without executing tasks"
    "Checks the compiled manifest without executing tasks or materialising runtime state."
  )
  (control "ps" "ps" "Reconcile and report Nixfied-owned processes for a slot"
    "Observes processes and reconciles stale registry evidence for the selected slot."
  )
  (control "down" "down" "Stop Nixfied-owned processes for a slot"
    "Stops owned processes and updates registry evidence for the selected slot."
  )
  (control "clean" "clean" "Safely clean Nixfied-owned state for a slot"
    "Performs marker-, policy-, lease- and process-gated cleanup of owned slot state."
  )
]
