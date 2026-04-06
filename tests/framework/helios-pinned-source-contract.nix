{ pkgs }:
let
  conf = import ../../nixfied/project/conf.nix { inherit pkgs; };
  inherit (conf.services) helios;
  inherit (conf.services) postgres;
  inherit (conf.services) minio;
  heliosHasNixpkgsSource = helios.sources ? nixpkgs;
in
assert helios.sources ? pinned;
assert helios.sourceKinds ? pinned;
assert (helios.sources.pinned.package or null) == null;
assert helios.sources.pinned.packageFactory == ../../nixfied/project/sources/helios-pinned.nix;
assert heliosHasNixpkgsSource == (pkgs ? helios);
assert if heliosHasNixpkgsSource then helios.sources.nixpkgs.packageAttr == "helios" else true;
assert postgres.sources.nixpkgs.packageAttr == "postgresql_16";
assert minio.sources.nixpkgs.packageAttr == "minio";
assert minio.sources.nixpkgs.clientPackageAttr == "minio-client";
assert helios.defaultSource == "pinned";
pkgs.runCommand "helios-pinned-source-contract" { } ''
  echo "OK: default service sources use lazy descriptors and helios stays pinned by default" > "$out"
''
