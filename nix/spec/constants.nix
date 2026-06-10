# The runtime ABI is derived: its suffix is a digest of the capability
# descriptor, the single manifest of the wire contract the runtime understands.
# The Rust side (nixfied-model::constants) derives the same value from the same
# file with the same SHA-256, so producer and consumer cannot disagree on the ABI
# and a contract change rotates it on both sides at once.
let
  capabilityDescriptor = builtins.readFile ../../runtime/crates/nixfied-model/capability.txt;
  capabilityDigest = builtins.substring 0 12 (builtins.hashString "sha256" capabilityDescriptor);
in
{
  modelVersion = 1;
  toolchainId = "nixfied-toolchain:1";
  runtimeAbi = "nixfied-runtime-abi:1-${capabilityDigest}";
}
