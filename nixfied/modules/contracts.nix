{ lib }:
{
  contractOptions = import ./lib/contract-options.nix { inherit lib; };
  apiOptions = import ./lib/api-options.nix { inherit lib; };
}
