{ lib }:
{ projectRoot, resolved }:
let
  statePolicyLib = import ./state-policy-lib.nix { inherit lib; };
in
statePolicyLib.compilePolicy {
  inherit projectRoot;
  inherit (resolved) identity;
  inherit (resolved.state) policy;
}
