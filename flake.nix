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

        project = import ./nix/project { inherit pkgs; };
        slots = import ./nix/slots.nix { inherit pkgs project; };

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

        hooks = import ./nix/hooks.nix {
          inherit
            pkgs
            project
            slots
            postgres
            nginx
            ;
        };

        lib = import ./nix/lib.nix { inherit pkgs project hooks; };
        supervisor = import ./nix/supervisor.nix { inherit pkgs project slots; };

        coreApps = import ./nix/apps/core.nix {
          inherit
            pkgs
            project
            lib
            ;
        };
        installApps = import ./nix/apps/install.nix {
          inherit
            pkgs
            lib
            ;
          frameworkRoot = ./.;
        };
        ciApp = import ./nix/ci.nix {
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
          // (if ciApp != null then { ci = ciApp; } else { })
          // installApps
          // {
            default =
              if coreApps ? help then coreApps.help else coreApps.dev;
          };

        packages = project.packages or { };
      }
    );
}
