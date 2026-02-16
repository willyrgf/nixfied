{ ... }:

{
  isolation = {
    enable = true;
    runCommand = "nix run .#ci -- --summary";
  };
}
