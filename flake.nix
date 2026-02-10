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

        project = import ./nixfied/project { inherit pkgs; };
        slots = import ./nixfied/slots.nix { inherit pkgs project; };

        postgres =
          if (project.modules.postgres.enable or false) then
            import ./nixfied/postgres { inherit pkgs project slots; }
          else
            null;

        nginx =
          if (project.modules.nginx.enable or false) then
            import ./nixfied/nginx { inherit pkgs project slots; }
          else
            null;

        playwright =
          if (project.modules.playwright.enable or false) then
            import ./nixfied/playwright.nix { inherit pkgs project; }
          else
            null;

        ephemeral =
          if (project.ephemeral.enable or false) then
            import ./nixfied/ephemeral.nix { inherit pkgs project; }
          else
            null;

        hooks = import ./nixfied/hooks.nix {
          inherit
            pkgs
            project
            slots
            postgres
            nginx
            supervisor
            ephemeral
            ;
        };

        lib = import ./nixfied/lib { inherit pkgs project hooks; };
        supervisor = import ./nixfied/supervisor { inherit pkgs project slots; };

        coreApps = import ./nixfied/internal/core.nix {
          inherit
            pkgs
            project
            lib
            moduleApps
            ;
        };
        isFramework = builtins.pathExists ./nixfied/.framework;

        installApps =
          if isFramework then
            import ./nixfied/internal/install.nix {
              inherit
                pkgs
                lib
                ;
              frameworkRoot = ./.;
            }
          else
            { };

        testApps =
          if isFramework then
            import ./nixfied/internal/test.nix {
              inherit
                pkgs
                lib
                ;
            }
          else
            { };
        isolationApps = import ./nixfied/internal/isolation.nix {
          inherit
            pkgs
            project
            lib
            slots
            ;
        };
        moduleApps = import ./nixfied/internal/module-apps.nix {
          inherit
            pkgs
            project
            lib
            postgres
            nginx
            supervisor
            slots
            ;
        };
        frameworkApps = pkgs.lib.mapAttrs' (name: value: {
          name = "framework::${name}";
          value = value;
        }) (installApps // testApps);
        ciEntry = import ./nixfied/ci.nix {
          inherit
            pkgs
            project
            lib
            ephemeral
            ;
        };
        ciApp =
          if ciEntry == null then
            null
          else if ciEntry ? app then
            ciEntry.app
          else
            ciEntry;
      in
      {
        devShells.default = import ./nixfied/devshell.nix {
          inherit
            pkgs
            project
            ;
        };

        apps =
          coreApps
          // moduleApps
          // (if ciApp != null then { ci = ciApp; } else { })
          // isolationApps
          // frameworkApps
          // {
            default = if coreApps ? help then coreApps.help else coreApps.dev;
          };

        packages = project.packages or { };
      }
    );
}
