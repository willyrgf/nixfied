{
  pkgs,
  model,
  stateHash,
  canonical,
  registry,
}:
{
  "v2-model-hash" = import ./model-hash.nix {
    inherit
      pkgs
      model
      stateHash
      canonical
      ;
  };

  "v2-cross-machine-hash" = import ./cross-machine-hash.nix {
    inherit
      pkgs
      model
      stateHash
      canonical
      ;
  };

  "v2-scheduler-order" = import ./scheduler-order.nix {
    inherit
      pkgs
      model
      ;
  };

  "v2-help-snapshot" = import ./help-snapshot.nix {
    inherit
      pkgs
      model
      ;
  };

  "v2-registry-replay" = import ./registry-replay.nix {
    inherit
      pkgs
      registry
      ;
  };

  "v2-compiler-validation" = import ./compiler-validation.nix {
    inherit
      pkgs
      model
      ;
  };

  "v2-executor-contract" = import ./executor-contract.nix {
    inherit pkgs;
  };

  "v2-env-sandbox-contract" = import ./env-sandbox-contract.nix {
    inherit pkgs;
  };

  "v2-registry-events-contract" = import ./registry-events-contract.nix {
    inherit
      pkgs
      registry
      ;
  };

  "v2-log-prefix-contract" = import ./log-prefix-contract.nix {
    inherit pkgs;
  };
}
