{ }:

{
  kernelExportRuntime = ''
    nixfied_load_kernel_exports() {
      local export_prefix="$1"
      local export_file=""
      shift || true

      if [ -z "$export_prefix" ] || [ "$#" -eq 0 ]; then
        echo "usage: nixfied_load_kernel_exports <export-prefix> <command...>" >&2
        return 1
      fi

      export_file="$(mktemp "''${TMPDIR:-/tmp}/$export_prefix.XXXXXX")" || return 1
      if ! "$@" "$export_file" >/dev/null; then
        rm -f "$export_file"
        return 1
      fi
      if [ ! -f "$export_file" ]; then
        rm -f "$export_file"
        return 1
      fi
      if ! . "$export_file"; then
        rm -f "$export_file"
        return 1
      fi
      rm -f "$export_file"
    }
  '';
}
