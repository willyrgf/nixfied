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
            import ./nix/postgres { inherit pkgs project slots; }
          else
            null;

        nginx =
          if (project.modules.nginx.enable or false) then
            import ./nix/nginx { inherit pkgs project slots; }
          else
            null;

        playwright =
          if (project.modules.playwright.enable or false) then
            import ./nix/playwright.nix { inherit pkgs project; }
          else
            null;

        ephemeral =
          if (project.ephemeral.enable or false) then
            import ./nix/ephemeral.nix { inherit pkgs project; }
          else
            null;

        hooks = import ./nix/hooks.nix {
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

        lib = import ./nix/lib { inherit pkgs project hooks; };
        supervisor = import ./nix/supervisor { inherit pkgs project slots; };

        coreApps = import ./nix/apps/core.nix {
          inherit
            pkgs
            project
            lib
            moduleApps
            ;
        };
        isFramework = builtins.pathExists ./nix/.framework;

        installApps =
          if isFramework then
            import ./nix/apps/install.nix {
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
            import ./nix/apps/test.nix {
              inherit
                pkgs
                lib
                ;
            }
          else
            { };
        isolationApps = import ./nix/apps/isolation.nix {
          inherit
            pkgs
            project
            lib
            slots
            ;
        };
        moduleApps = import ./nix/apps/module-apps.nix {
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
        ciEntry = import ./nix/ci.nix {
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
        devShells.default = import ./nix/devshell.nix {
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
