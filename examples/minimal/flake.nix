{
  description = "Nixfied M0 minimal downstream-shaped example";

  inputs = {
    nixfied.url = "../..";
  };

  outputs =
    { self, nixfied }:
    let
      systems = [
        "aarch64-darwin"
        "aarch64-linux"
        "x86_64-linux"
      ];
      forAllSystems =
        f:
        builtins.listToAttrs (
          map (system: {
            name = system;
            value = f system;
          }) systems
        );
    in
    {
      packages = forAllSystems (system: {
        default = nixfied.lib.${system}.compileManifest ./nixfied.nix;
        manifest = nixfied.lib.${system}.compileManifest ./nixfied.nix;
      });
    };
}
