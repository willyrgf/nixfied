{ }:

{
  kernelExportRuntime = ''
    nixfied_load_kernel_exports() {
      local export_prefix="$1"
      local export_text=""
      shift || true

      if [ -z "$export_prefix" ] || [ "$#" -eq 0 ]; then
        echo "usage: nixfied_load_kernel_exports <export-prefix> <command...>" >&2
        return 1
      fi

      if ! export_text="$("$@")"; then
        return 1
      fi

      if [ -z "$export_text" ]; then
        return 1
      fi

      if ! eval "$export_text"; then
        return 1
      fi
    }
  '';
}
