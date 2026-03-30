{
  pkgs,
  project ? { },
  loggingPrelude ? null,
}:

let
  policy = import ./runtime-event-policy.nix {
    inherit pkgs project;
  };
  programs = import ../runtime/runtime-events-programs.nix {
    inherit
      pkgs
      project
      loggingPrelude
      policy
      ;
  };
in
programs
// {
  inherit policy;
  inherit (policy) registryRoot;
}
