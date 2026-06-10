{
  description = "Nixfied downstream worked example";

  inputs = {
    nixfied.url = "../..";
  };

  outputs =
    { self, nixfied }:
    let
      systems = [
        "aarch64-darwin"
        "aarch64-linux"
        "x86_64-darwin"
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
        default = nixfied.lib.${system}.compileModel ./nixfied.nix;
        model = nixfied.lib.${system}.compileModel ./nixfied.nix;
      });
      # The generated run/check/test/ci surface (see this example's README).
      apps = forAllSystems (system: nixfied.lib.${system}.projectApps ./nixfied.nix);
    };
}
