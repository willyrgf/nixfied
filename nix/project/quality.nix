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
      env = { };
      useDeps = true;
      script = ''
        echo "Quality checks placeholder. Edit nix/project/quality.nix."
        exit 0
      '';
    };
  };
}
