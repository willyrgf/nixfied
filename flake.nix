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
            import ./nix/apps/test-framework.nix {
              inherit
                pkgs
                lib
                ;
            }
          else
            { };
        frameworkApps = pkgs.lib.mapAttrs' (name: value: {
          name = if name == "test-framework" then "framework::test" else "framework::${name}";
          value = value;
        }) (installApps // testApps);
        ciEntry = import ./nix/ci.nix {
          inherit
            pkgs
            project
            lib
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
          // (if ciApp != null then { ci = ciApp; } else { })
          // frameworkApps
          // {
            default = if coreApps ? help then coreApps.help else coreApps.dev;
          };

        packages = project.packages or { };
      }
    );
}
