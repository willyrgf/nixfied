{ project, ... }:

{
  # Example (uncomment and adapt):
  # commands.check.script = ''
  #   ./lint
  #   ./typecheck
  # '';

  commands = {
    check = {
      description = "Run quality checks";
      api = {
        version = 1;
        summary = "Run quality checks";
        details = ''
          Runs the project's quality checks (lint, typecheck, format checks, etc).

          Customize this command in nixfied/project/quality.nix.
        '';
        usage = [ "nix run .#check" ];
        examples = [ "nix run .#check" ];
        category = "core";
      };
      env = { };
      useDeps = true;
      script = ''
        echo "Quality checks placeholder. Edit nixfied/project/quality.nix."
        exit 0
      '';
    };
  };
}
