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
}
