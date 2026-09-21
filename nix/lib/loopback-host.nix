# Native Nix authoring accepts canonical loopback forms. Rust's LoopbackHost
# independently accepts loopback IP literals at deserialization.
{ lib }:
host:
host == "::1"
|| (
  let
    octets = builtins.match "127\\.([0-9]+)\\.([0-9]+)\\.([0-9]+)" host;
  in
  octets != null && lib.all (octet: lib.toInt octet <= 255) octets
)
