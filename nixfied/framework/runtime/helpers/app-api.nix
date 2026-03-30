{
  pkgs,
  shellContract ? import ./shell-contract.nix { inherit pkgs; },
  mkApp ? null,
}:
let
  commandApi = import ../../core/command-api.nix {
    inherit pkgs;
  };
  inherit (commandApi) mkCommandApi;

  mkNixfiedApp =
    {
      name,
      script,
      fixtures ? null,
      env ? { },
      useDeps ? false,
      fixtureProfile ? "default",
      description ? null,
      meta ? { },
      api ? null,
    }:
    if mkApp == null then
      throw "mkNixfiedApp requires mkApp"
    else
    mkApp {
      inherit
        name
        script
        fixtures
        env
        useDeps
        fixtureProfile
        description
        meta
        api
        ;
    };

  mkContractBackedApp =
    {
      name,
      script,
      contract,
      fixtures ? null,
      env ? { },
      useDeps ? false,
      fixtureProfile ? "default",
      description ? null,
      meta ? { },
    }:
    if mkApp == null then
      throw "mkContractBackedApp requires mkApp"
    else
    mkNixfiedApp {
      inherit
        name
        script
        fixtures
        env
        useDeps
        fixtureProfile
        description
        meta
        ;
      api = mkCommandApi {
        class = contract.class or "typed";
        inherit name;
        summary = contract.summary;
        details = contract.details or "";
        usage = contract.usage or [ ];
        examples = contract.examples or [ ];
        args = contract.args or [ ];
        env = contract.env or [ ];
        category = contract.category or "core";
        idempotent = contract.idempotent or false;
        outputs = contract.outputs or null;
      };
    };
in
{
  inherit
    mkCommandApi
    mkNixfiedApp
    mkContractBackedApp
    ;
}
