{
  pkgs,
  model,
  stateHash,
  canonical,
  registry,
}:
{
  "model-hash" = import ./model-hash.nix {
    inherit
      pkgs
      model
      stateHash
      canonical
      ;
  };

  "cross-machine-hash" = import ./cross-machine-hash.nix {
    inherit
      pkgs
      model
      stateHash
      canonical
      ;
  };

  "scheduler-order" = import ./scheduler-order.nix {
    inherit
      pkgs
      model
      ;
  };

  "help-snapshot" = import ./help-snapshot.nix {
    inherit
      pkgs
      model
      ;
  };

  "registry-replay" = import ./registry-replay.nix {
    inherit
      pkgs
      registry
      ;
  };

  "compiler-validation" = import ./compiler-validation.nix {
    inherit
      pkgs
      model
      ;
  };

  "executor-contract" = import ./executor-contract.nix {
    inherit pkgs;
  };

  "env-sandbox-contract" = import ./env-sandbox-contract.nix {
    inherit pkgs;
  };

  "registry-events-contract" = import ./registry-events-contract.nix {
    inherit
      pkgs
      registry
      ;
  };

  "log-prefix-contract" = import ./log-prefix-contract.nix {
    inherit pkgs;
  };

  "parallel-runner-smoke" = import ./parallel-runner-smoke.nix {
    inherit
      pkgs
      model
      registry
      ;
  };

  "task-hooks-smoke" = import ./task-hooks-smoke.nix {
    inherit
      pkgs
      model
      registry
      ;
  };

  "framework-install-vendor-smoke" = import ./framework-install-vendor-smoke.nix {
    inherit
      pkgs
      model
      registry
      ;
  };
}
