{ pkgs }:
let
  kernelPackage = import ../../nixfied/framework/runtime/kernel { inherit pkgs; };
in
pkgs.runCommand "kernel-native-tests" { } ''
  test -x ${kernelPackage}/bin/nixfied-kernel
  echo "OK: kernel package builds and native Rust tests passed" > "$out"
''
