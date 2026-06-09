# Nix-side adapters: libraries that compile concrete services into the generic
# model primitives. Exposed to every compiled module through `specialArgs` as
# `adapters`, so a downstream `nixfied.nix` can `imports = [ adapters.<name> ]`
# without vendoring framework internals.
{
  synthetic = import ./synthetic.nix;
}
