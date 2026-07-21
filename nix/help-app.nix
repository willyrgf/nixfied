{
  pkgs,
  system,
  flakeRef,
}:
let
  target = "${flakeRef}#apps.${system}";
  app = pkgs.writeShellApplication {
    name = "nixfied-help";
    text = ''
      case "$#" in
        0) ;;
        1)
          case "$1" in
            -h|--help) ;;
            *)
              echo "help: unknown argument: $1 (supported: -h, --help)" >&2
              exit 1
              ;;
          esac
          ;;
        *)
          echo "help: expected no arguments, -h, or --help" >&2
          exit 1
          ;;
      esac

      exec ${pkgs.nix}/bin/nix eval \
        --no-write-lock-file \
        --raw ${pkgs.lib.escapeShellArg target} \
        --apply "$(<${./help-renderer.nix})"
    '';
  };
in
{
  type = "app";
  program = "${app}/bin/nixfied-help";
  meta.description = "List this flake's runnable commands";
}
