{
  description = "Generic Nix project framework";

  inputs = {
    nixpkgs.url = "github:nixos/nixpkgs?ref=nixos-unstable";
    flake-utils.url = "github:numtide/flake-utils";
  };

  outputs =
    {
      self,
      nixpkgs,
      flake-utils,
    }:
    flake-utils.lib.eachDefaultSystem (
      system:
      let
        pkgs = import nixpkgs { inherit system; };

        project = import ./nix/project.nix { inherit pkgs; };
        slots = import ./nix/slots.nix { inherit pkgs project; };
        lib = import ./nix/lib.nix { inherit pkgs project; };
        supervisor = import ./nix/supervisor.nix { inherit pkgs project slots; };

        postgres =
          if (project.modules.postgres.enable or false) then
            import ./nix/postgres.nix { inherit pkgs project slots; }
          else
            null;

        nginx =
          if (project.modules.nginx.enable or false) then
            import ./nix/nginx.nix { inherit pkgs project slots; }
          else
            null;

        playwright =
          if (project.modules.playwright.enable or false) then
            import ./nix/playwright.nix { inherit pkgs project; }
          else
            null;

        coreApps = import ./nix/apps/core.nix {
          inherit
            pkgs
            project
            lib
            ;
        };
      in
      {
        devShells.default = import ./nix/devshell.nix {
          inherit
            pkgs
            project
            ;
        };

        apps =
          coreApps
          // {
            default =
              if coreApps ? help then coreApps.help else coreApps.dev;
          };

        packages = project.packages or { };
      }
    );
}
