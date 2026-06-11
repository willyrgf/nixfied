use std::sync::LazyLock;

use sha2::{Digest, Sha256};

pub const MODEL_VERSION: u32 = 1;
pub const TOOLCHAIN_ID: &str = "nixfied-toolchain:1";

/// The semantic prefix of the runtime ABI. The full ABI appends a digest of the
/// capability descriptor, so a contract change rotates it automatically.
const RUNTIME_ABI_BASE: &str = "nixfied-runtime-abi:1";

/// The declared wire-contract surface the runtime understands: the admitted
/// primitives, command surfaces, stop signals, and registry status domains.
/// Editing it is how a contract change is recorded; its digest is baked into the
/// runtime ABI (and into the Nix-emitted `runtimeAbi`), so an out-of-date model
/// fails the identity check instead of being silently mis-executed.
pub const CAPABILITY_DESCRIPTOR: &str = include_str!("../capability.txt");

/// The first 12 hex chars of the capability descriptor's SHA-256. The Nix emitter
/// derives the same value from the same file, so the two sides cannot disagree.
pub fn capability_digest() -> &'static str {
    static DIGEST: LazyLock<String> = LazyLock::new(|| {
        hex::encode(Sha256::digest(CAPABILITY_DESCRIPTOR.as_bytes()))[..12].to_string()
    });
    DIGEST.as_str()
}

/// The runtime ABI, `nixfied-runtime-abi:1-<capabilityDigest>`.
pub fn runtime_abi() -> &'static str {
    static ABI: LazyLock<String> =
        LazyLock::new(|| format!("{RUNTIME_ABI_BASE}-{}", capability_digest()));
    ABI.as_str()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Snapshot of the derived ABI. Changing the capability descriptor rotates the
    /// digest and breaks this assertion: update it deliberately, in the same change
    /// that records the contract change, and confirm the Nix `runtimeAbi` matches
    /// (the gate checks producer/consumer agreement).
    #[test]
    fn runtime_abi_snapshot() {
        assert_eq!(runtime_abi(), "nixfied-runtime-abi:1-73f36f09b811");
    }

    #[test]
    fn capability_digest_is_twelve_hex_chars() {
        let digest = capability_digest();
        assert_eq!(digest.len(), 12);
        assert!(digest.bytes().all(|b| b.is_ascii_hexdigit()));
    }
}
