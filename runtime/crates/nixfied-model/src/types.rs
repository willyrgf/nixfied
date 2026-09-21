use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::ids::{ClosureId, CodebaseId, OperationId, ServiceId, TaskId};
use crate::unique_vec::UniqueVec;

include!("generated/types.rs");

/// A host that is provably an IP loopback literal — `"localhost"` and `"0.0.0.0"`
/// cannot deserialize, so the host-parse failure is unrepresentable at the wire.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LoopbackHost(std::net::IpAddr);

impl LoopbackHost {
    /// Construct from a string in code (e.g. tests); the same loopback check the
    /// `Deserialize` impl applies.
    pub fn parse(host: &str) -> Result<Self, String> {
        let ip: std::net::IpAddr = host
            .parse()
            .map_err(|_| format!("host {host} is not an IP literal"))?;
        if !ip.is_loopback() {
            return Err(format!("host {host} is not a loopback address"));
        }
        Ok(Self(ip))
    }

    pub fn ip(&self) -> std::net::IpAddr {
        self.0
    }
}

impl std::fmt::Display for LoopbackHost {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.0.fmt(f)
    }
}

impl Serialize for LoopbackHost {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.collect_str(&self.0)
    }
}

impl<'de> Deserialize<'de> for LoopbackHost {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let raw = String::deserialize(deserializer)?;
        let ip: std::net::IpAddr = raw
            .parse()
            .map_err(|_| serde::de::Error::custom(format!("host {raw} is not an IP literal")))?;
        if !ip.is_loopback() {
            return Err(serde::de::Error::custom(format!(
                "host {raw} is not a loopback address"
            )));
        }
        Ok(Self(ip))
    }
}

// The native convenience default is separate from the wire field's default.
#[allow(clippy::derivable_impls)]
impl Default for TaskDefaultOutput {
    fn default() -> Self {
        Self::Summary
    }
}
