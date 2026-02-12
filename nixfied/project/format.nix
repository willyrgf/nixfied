{ ... }:

{
  commands = {
    format = {
      description = "Format Nix files";
      api = {
        version = 1;
        summary = "Format Nix files";
        details = ''
          Formats all *.nix files in the repository using nixfmt.

          Use this after making Nix changes.
        '';
        usage = [ "nix run .#format" ];
        examples = [ "nix run .#format" ];
        category = "core";
      };
      env = { };
      useDeps = false;
      script = ''
        find . -name '*.nix' -print0 | xargs -0 nixfmt --
      '';
    };
  };
}
