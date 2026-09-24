{ pkgs, apps }:
let
  catalog = pkgs.writeText "nixfied-help-catalog" (import ./help-renderer.nix apps);
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

      exec ${pkgs.coreutils}/bin/cat ${catalog}
    '';
  };
in
{
  type = "app";
  program = "${app}/bin/nixfied-help";
}
