use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
/// Represents `CompatibilityTarget` data exposed by this crate.
pub struct CompatibilityTarget {
    pub name: String,
    pub status: String,
}
